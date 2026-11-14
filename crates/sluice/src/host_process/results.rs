//! The host process's results index, one for each repository it holds
//! (69, 129; design D17; record 0093 as amended 2026-11-13). It reads the
//! repository's results log from the byte offset it last read, the whole
//! log again where the file shrank or was replaced, and each signer's ref
//! by entry, a blob not yet seen verified once with the key the asking
//! branch's policy lists for its signer and kept by blob, signer and key
//! digest, as the records profile keeps them for a command. Lines are kept
//! by key, combination and task, so an ask is answered from them with no
//! line of the log read again, however long the log grows. It lives for
//! the host process's life; nothing it keeps decides anything between
//! steps, and nothing takes a result away (69, 109; rule core.14).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::Deserialize;
use sluice_core::records::{to_line, ResultLine};
use sluice_seams::ResultAsk;

/// A signer's ref as the host last fetched it, as the asking branch's
/// policy lists the signer: what the index reads it through.
pub trait SignerRef {
    /// The signer's name.
    fn signer(&self) -> &str;

    /// A digest of the key the asking branch's policy lists for the
    /// signer, which tells it from another key the signer is listed with
    /// elsewhere (78; rule core.22).
    fn digest(&self) -> &str;

    /// Each entry of the ref as last fetched: its path and its blob's id,
    /// in the ref's order. None where the host holds no such ref.
    fn entries(&self) -> Result<Vec<(String, String)>>;

    /// The blob `oid`'s bytes.
    fn blob(&self, oid: &str) -> Result<Vec<u8>>;

    /// The results the statement `bytes` signs, verified with the key the
    /// policy lists; an error where it does not verify.
    fn verify(&self, bytes: &[u8]) -> Result<Vec<ResultLine>>;
}

/// The index of one repository's results, `Send + Sync` behind its mutex:
/// shared by the repository's task and the connection threads that answer
/// `results` (design D15, D17).
pub struct Index(Mutex<Inner>);

struct Inner {
    /// The results log.
    log: PathBuf,
    /// Where the log was last read to: past its last whole line.
    offset: u64,
    /// The file the offset is into, by its device and inode: a log
    /// replaced is read whole again.
    file: Option<(u64, u64)>,
    /// The host's own lines.
    own: Lines,
    /// Each statement verified, by its blob, the signer it was read for
    /// and the digest of the key it was verified with; `None` where it did
    /// not verify, so it is never verified with that key again.
    verified: BTreeMap<(String, String, String), Option<Lines>>,
    /// How many bytes of the log were read, all told.
    read: u64,
    /// How many statements were verified, all told.
    verifications: u64,
}

/// What a line of the results log is kept by.
#[derive(Deserialize)]
struct Keyed {
    #[serde(default)]
    task: String,
    #[serde(default)]
    hash: String,
    #[serde(default)]
    combination: String,
}

/// Lines in their order, kept by key, combination and task.
#[derive(Default)]
struct Lines {
    lines: Vec<Vec<u8>>,
    by_key: BTreeMap<(String, String), Vec<usize>>,
    by_combination: BTreeMap<String, Vec<usize>>,
    by_task: BTreeMap<String, Vec<usize>>,
}

impl Lines {
    /// Keep `line`, one JSON document with its newline. A line that names
    /// no task, hash and combination it can read is kept for an ask of
    /// every line alone.
    fn push(&mut self, line: &[u8]) {
        let at = self.lines.len();
        self.lines.push(line.to_vec());
        let Ok(k) = serde_json::from_slice::<Keyed>(line) else {
            return;
        };
        self.by_key
            .entry((k.task.clone(), k.hash))
            .or_default()
            .push(at);
        self.by_combination
            .entry(k.combination)
            .or_default()
            .push(at);
        self.by_task.entry(k.task).or_default().push(at);
    }

    /// The lines `ask` names, in their order, each once.
    fn answer(&self, ask: &ResultAsk, out: &mut Vec<u8>) {
        if ask.is_every() {
            for l in &self.lines {
                out.extend_from_slice(l);
            }
            return;
        }
        let mut at: Vec<usize> = Vec::new();
        for k in &ask.keys {
            at.extend(self.by_key.get(k).into_iter().flatten());
        }
        for c in &ask.combinations {
            at.extend(self.by_combination.get(&c.0).into_iter().flatten());
        }
        for t in &ask.tasks {
            at.extend(self.by_task.get(t).into_iter().flatten());
        }
        at.sort_unstable();
        at.dedup();
        for i in at {
            out.extend_from_slice(&self.lines[i]);
        }
    }
}

impl Index {
    /// The index of the results log at `log`, read at the first ask.
    pub fn new(log: &Path) -> Index {
        Index(Mutex::new(Inner {
            log: log.to_path_buf(),
            offset: 0,
            file: None,
            own: Lines::default(),
            verified: BTreeMap::new(),
            read: 0,
            verifications: 0,
        }))
    }

    /// The lines `ask` names, one JSON document per line: the host's own,
    /// read from the log where it was last read to, then each signer's of
    /// `signed`, read from its ref by entry, a blob first seen with a key
    /// verified once with it, in the order `signed` gives them (78; rule
    /// core.22). An ask for nothing reads nothing.
    pub fn answer(&self, ask: &ResultAsk, signed: &[&dyn SignerRef]) -> Result<Vec<u8>> {
        let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::new();
        if ask.is_empty() {
            return Ok(out);
        }
        inner.catch_up()?;
        inner.own.answer(ask, &mut out);
        for s in signed {
            for (path, oid) in s.entries()? {
                let seen = (oid.clone(), s.signer().to_string(), s.digest().to_string());
                if !inner.verified.contains_key(&seen) {
                    let lines = match s.verify(&s.blob(&oid)?) {
                        Ok(lines) => {
                            let mut kept = Lines::default();
                            for l in &lines {
                                kept.push(&to_line(l));
                            }
                            Some(kept)
                        }
                        Err(e) => {
                            tracing::warn!(
                                sluice.signer = s.signer(),
                                sluice.path = %path,
                                "a run record that does not verify is left out: {e:#}"
                            );
                            None
                        }
                    };
                    inner.verifications += 1;
                    inner.verified.insert(seen.clone(), lines);
                }
                if let Some(Some(lines)) = inner.verified.get(&seen) {
                    lines.answer(ask, &mut out);
                }
            }
        }
        Ok(out)
    }

    /// How many bytes of the log it has read, all told.
    pub fn bytes_read(&self) -> u64 {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).read
    }

    /// How many statements it has verified, all told.
    pub fn verifications(&self) -> u64 {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .verifications
    }
}

impl Inner {
    /// Read what the log holds past the offset, whole lines alone; the
    /// whole log again where it shrank below the offset or is another
    /// file than the one the offset is into.
    fn catch_up(&mut self) -> Result<()> {
        let mut f = match File::open(&self.log) {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                self.forget();
                return Ok(());
            }
            Err(e) => return Err(e).context("reading the results log"),
        };
        let meta = f.metadata().context("reading the results log")?;
        let file = (meta.dev(), meta.ino());
        if self.file != Some(file) || meta.len() < self.offset {
            self.forget();
            self.file = Some(file);
        }
        if meta.len() == self.offset {
            return Ok(());
        }
        f.seek(SeekFrom::Start(self.offset))
            .context("reading the results log")?;
        let mut more = Vec::new();
        f.read_to_end(&mut more)
            .context("reading the results log")?;
        self.read += more.len() as u64;
        // A line still being written, or one a crash cut short, waits for
        // its end.
        let Some(end) = more.iter().rposition(|b| *b == b'\n').map(|i| i + 1) else {
            return Ok(());
        };
        for line in more[..end].split_inclusive(|b| *b == b'\n') {
            if line != b"\n" {
                self.own.push(line);
            }
        }
        self.offset += end as u64;
        Ok(())
    }

    /// Forget the log's lines: it is read whole again.
    fn forget(&mut self) {
        self.offset = 0;
        self.file = None;
        self.own = Lines::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::io::Write;

    /// A scratch folder, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!(
                "sluice-results-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A result of `task` at `hash` on `combination`, as a line.
    fn line(task: &str, hash: &str, combination: &str) -> String {
        format!(
            "{{\"at\":\"2026-11-13T00:00:00Z\",\"kind\":\"ran\",\"task\":\"{task}\",\
             \"hash\":\"{hash}\",\"outcome\":\"pass\",\"branch\":\"main\",\
             \"combination\":\"{combination}\"}}\n"
        )
    }

    fn append(path: &Path, text: &str) {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    fn every(index: &Index) -> String {
        String::from_utf8(index.answer(&ResultAsk::every(), &[]).unwrap()).unwrap()
    }

    /// 69 (design D17): a result appended is read from where the log was
    /// last read to, and a line not yet whole waits for its end; lines are
    /// answered by key, combination and task.
    #[test]
    fn a_result_appended_is_read_from_the_offset() {
        let dir = Scratch::new("offset");
        let log = dir.0.join("results.jsonl");
        let (a, b, c) = (
            line("a:test", "h1", "c1"),
            line("b:test", "h2", "c1"),
            line("c:test", "h3", "c2"),
        );
        append(&log, &format!("{a}{b}"));
        let index = Index::new(&log);
        // Shared by the repository's task and the connection threads.
        fn shared<T: Send + Sync>(_: &T) {}
        shared(&index);
        assert_eq!(every(&index), format!("{a}{b}"));
        let first = index.bytes_read();
        assert_eq!(first, (a.len() + b.len()) as u64);

        let (whole, cut) = c.split_at(20);
        append(&log, whole);
        assert_eq!(every(&index), format!("{a}{b}"), "a line not yet whole");
        append(&log, cut);
        assert_eq!(every(&index), format!("{a}{b}{c}"));
        assert_eq!(
            index.bytes_read(),
            first + whole.len() as u64 + c.len() as u64,
            "read from the offset, the cut line read again from its start"
        );
        // Asked again with nothing appended: nothing read.
        let before = index.bytes_read();
        every(&index);
        assert_eq!(index.bytes_read(), before);

        let ask = |ask: ResultAsk| String::from_utf8(index.answer(&ask, &[]).unwrap()).unwrap();
        assert_eq!(ask(ResultAsk::default().key("b:test", "h2")), b);
        assert_eq!(ask(ResultAsk::default().key("b:test", "h1")), "");
        assert_eq!(
            ask(ResultAsk::default().combination(&sluice_seams::Rev("c1".into()))),
            format!("{a}{b}")
        );
        assert_eq!(
            ask(ResultAsk::default().task("c:test").key("a:test", "h1")),
            format!("{a}{c}"),
            "in the log's order, each once"
        );
        assert_eq!(ask(ResultAsk::default()), "");
    }

    /// 69 (design D17): a log that shrank, or that another file replaced,
    /// is read whole again, and no line of the one before is answered.
    #[test]
    fn a_log_replaced_is_read_whole_again() {
        let dir = Scratch::new("replaced");
        let log = dir.0.join("results.jsonl");
        let (a, b, c) = (
            line("a:test", "h1", "c1"),
            line("b:test", "h2", "c1"),
            line("c:test", "h3", "c2"),
        );
        append(&log, &format!("{a}{b}"));
        let index = Index::new(&log);
        assert_eq!(every(&index), format!("{a}{b}"));

        // Replaced, longer than it was.
        let aside = dir.0.join("results.new");
        std::fs::write(&aside, format!("{c}{a}{b}")).unwrap();
        std::fs::rename(&aside, &log).unwrap();
        assert_eq!(every(&index), format!("{c}{a}{b}"));

        // Shrunk in place.
        std::fs::OpenOptions::new()
            .write(true)
            .open(&log)
            .unwrap()
            .set_len(c.len() as u64)
            .unwrap();
        assert_eq!(every(&index), c);

        // Gone, then written again.
        std::fs::remove_file(&log).unwrap();
        assert_eq!(every(&index), "");
        append(&log, &b);
        assert_eq!(every(&index), b);
    }

    /// A signer's ref of `entries`, each blob a statement of one result,
    /// verified with the key `digest`, counting its verifications.
    struct Fake {
        digest: String,
        entries: Vec<(String, String)>,
        verified: Cell<usize>,
    }

    impl SignerRef for Fake {
        fn signer(&self) -> &str {
            "a"
        }

        fn digest(&self) -> &str {
            &self.digest
        }

        fn entries(&self) -> Result<Vec<(String, String)>> {
            Ok(self.entries.clone())
        }

        fn blob(&self, oid: &str) -> Result<Vec<u8>> {
            Ok(oid.as_bytes().to_vec())
        }

        fn verify(&self, bytes: &[u8]) -> Result<Vec<ResultLine>> {
            self.verified.set(self.verified.get() + 1);
            let oid = std::str::from_utf8(bytes)?;
            if oid == "forged" {
                anyhow::bail!("the signature does not verify");
            }
            Ok(vec![serde_json::from_str(&line(oid, "h", "c1"))?])
        }
    }

    /// 78 (design D17; rule core.22): a blob is verified once for each key
    /// it is read with: asked again with the same key, nothing is verified
    /// again; with another key the signer is listed with elsewhere, each
    /// blob is verified with that key, once; one that does not verify is
    /// left out, and not verified with its key again.
    #[test]
    fn a_blob_is_verified_once_for_each_key() {
        let dir = Scratch::new("verified");
        let index = Index::new(&dir.0.join("results.jsonl"));
        let fake = |digest: &str, entries: &[&str]| Fake {
            digest: digest.to_string(),
            entries: entries
                .iter()
                .map(|e| (format!("runs/{e}"), e.to_string()))
                .collect(),
            verified: Cell::new(0),
        };
        let ask = ResultAsk::default().combination(&sluice_seams::Rev("c1".into()));
        let one = fake("k1", &["x:test", "forged", "y:test"]);
        let got = String::from_utf8(index.answer(&ask, &[&one]).unwrap()).unwrap();
        assert!(got.contains("x:test") && got.contains("y:test"), "{got}");
        assert_eq!(got.lines().count(), 2, "{got}");
        assert_eq!(one.verified.get(), 3);
        assert_eq!(index.answer(&ask, &[&one]).unwrap(), got.as_bytes());
        assert_eq!(one.verified.get(), 3, "nothing verified again");

        // A blob added to the ref is verified, the others not again.
        let more = fake("k1", &["x:test", "forged", "y:test", "z:test"]);
        let got = String::from_utf8(index.answer(&ask, &[&more]).unwrap()).unwrap();
        assert_eq!(got.lines().count(), 3, "{got}");
        assert_eq!(more.verified.get(), 1);

        // Another branch's key for the same signer: each blob once more.
        let other = fake("k2", &["x:test", "forged", "y:test", "z:test"]);
        index.answer(&ask, &[&other]).unwrap();
        index.answer(&ask, &[&other]).unwrap();
        assert_eq!(other.verified.get(), 4);
        assert_eq!(index.verifications(), 8);
    }
}
