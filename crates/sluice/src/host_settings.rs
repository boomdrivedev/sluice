//! The host's own settings as a process that outlives a command holds
//! them, and the look that tells it, between its passes, whether their
//! file changed (135; record 0107; rules core.31, core.54).
//!
//! [`HostSettings`] keeps the [`Instance`] the process works under and the
//! sign of the file it read it from. [`HostSettings::look`] takes the sign
//! again and compares: while it stands, nothing is read. The sign is three
//! parts, each `None` where there is nothing to read:
//!
//! - the file's own entry, not followed through a link: its modification
//!   time and size;
//! - the path the file resolves to through every link, a link to its
//!   folder among them;
//! - the file found there: its modification time and size.
//!
//! The link's entry and the resolved path see a link moved into a store
//! whose files all have one time, as sluice's own flake module makes the
//! file (core.51), where the target's time and size alone would miss it.
//! A plain file edited in place, or replaced by a rename, moves its own
//! time. Taking the sign starts no process and reads no git (core.31).
//!
//! A look whose sign moved reads the file, and counts the read only where
//! the sign stood through it, so a writer still at work leaves nothing
//! half read. A file that is gone, does not read or is refused leaves the
//! settings held, and is said once for that change: the sign it was read
//! at is kept, so it is read again only once it changes again (core.54).
//!
//! Settings handed over by a `sluice run` that started afresh
//! ([`HostSettings::handed`]) are held from the text it bound, under no
//! sign, so the first look reads the file once: the settings handed over
//! read as nothing to take up, and a change made meanwhile as a change
//! (design D4 of `a-kept-run-hands-over-what-it-cannot-take-up`).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Result;

use crate::instance::{config_path, config_path_in, home, host_cores, settings_text, Instance};

/// The settings a process holds, and the sign of the file it read them
/// from (135; record 0107).
pub struct HostSettings {
    path: Option<PathBuf>,
    /// The cores the host gives sluice, and all of them, as the settings
    /// were first read on (71).
    given: usize,
    all: usize,
    held: Instance,
    sign: Sign,
}

/// What a look at the settings file found.
#[derive(Debug)]
pub enum Look {
    /// Nothing to take up: the file stands, a read was caught mid-write,
    /// or it holds the settings held.
    Same,
    /// The file holds other settings, which bind the process once its
    /// caller has bound them and [`HostSettings::accept`]s them, and the
    /// text they were read from, which a hand-over carries so the process
    /// started afresh holds exactly what was bound (design D5).
    Changed(Instance, String),
    /// The changed file is gone or does not read, in these words; the
    /// settings held stand.
    Refused(String),
}

/// The file's sign: its own entry, the path it resolves to and the file
/// there (design D1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Sign {
    entry: Option<Stamp>,
    resolved: Option<PathBuf>,
    target: Option<Stamp>,
}

/// A file's modification time and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl Stamp {
    fn of(meta: std::io::Result<std::fs::Metadata>) -> Option<Stamp> {
        let meta = meta.ok()?;
        Some(Stamp {
            modified: meta.modified().ok(),
            len: meta.len(),
        })
    }
}

impl Sign {
    /// The sign of the file at `path`, with no process started and nothing
    /// read but its entries (core.31).
    fn of(path: Option<&Path>) -> Sign {
        let Some(path) = path else {
            return Sign::default();
        };
        Sign {
            entry: Stamp::of(std::fs::symlink_metadata(path)),
            resolved: std::fs::canonicalize(path).ok(),
            target: Stamp::of(std::fs::metadata(path)),
        }
    }
}

/// What a gone file is said with: the process keeps what it holds, and
/// an operator who wants the defaults writes an empty file (design D4).
const GONE: &str = "it is gone; an empty file gives sluice's defaults";

impl HostSettings {
    /// The host's settings, `$XDG_CONFIG_HOME/sluice/config.toml`, on the
    /// host's cores (71), read as a command reads them.
    pub fn read() -> Result<HostSettings> {
        let (given, all) = host_cores();
        HostSettings::at(config_path().as_deref(), given, all)
    }

    /// As [`HostSettings::read`], the file named in the environment `env`,
    /// the one sluice's host process started with, which a host process a
    /// test serves holds apart from the test's own.
    pub fn read_in(env: &[(std::ffi::OsString, std::ffi::OsString)]) -> Result<HostSettings> {
        let (given, all) = host_cores();
        let var = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        HostSettings::at(config_path_in(var).as_deref(), given, all)
    }

    /// As [`HostSettings::read`], from the settings at `path`, on a host of
    /// `all` cores that gives sluice `given` of them. The sign is taken
    /// before the read, so a file that moves meanwhile is read again at
    /// the first look (design D2).
    pub fn at(path: Option<&Path>, given: usize, all: usize) -> Result<HostSettings> {
        let sign = Sign::of(path);
        let held = Instance::from_host(path, given, all)?;
        Ok(HostSettings {
            path: path.map(Path::to_path_buf),
            given,
            all,
            held,
            sign,
        })
    }

    /// As [`HostSettings::read`], the settings a `sluice run` started
    /// afresh was handed in `text`, held as [`HostSettings::handed`] holds
    /// them (design D4).
    pub fn read_handed(text: &str) -> Result<HostSettings> {
        let (given, all) = host_cores();
        HostSettings::handed(text, config_path().as_deref(), given, all)
    }

    /// The settings a `sluice run` started afresh was handed, read from
    /// the `text` the run bound, on a host of `all` cores that gives sluice
    /// `given` of them, held under no sign: its first look reads the file
    /// at `path` once, so settings equal to these are nothing to take up, a
    /// change made meanwhile is taken up, and a file that does not read
    /// leaves these held, said once (135; record 0107; rule core.54;
    /// design D4).
    pub fn handed(
        text: &str,
        path: Option<&Path>,
        given: usize,
        all: usize,
    ) -> Result<HostSettings> {
        let held = Instance::from_text(text, path, given, all, home().as_deref())?;
        Ok(HostSettings {
            path: path.map(Path::to_path_buf),
            given,
            all,
            held,
            sign: Sign::default(),
        })
    }

    /// The settings the process works under.
    pub fn held(&self) -> &Instance {
        &self.held
    }

    /// The settings file, where the host has a place for one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Whether the settings file changed since it was last read, and what
    /// it holds if so (design D1, D2, D4). The file is read only once its
    /// sign moves, and the read counts only where the sign stood through
    /// it; otherwise the look finds nothing, and the next reads again.
    pub fn look(&mut self) -> Look {
        let path = self.path.as_deref();
        let before = Sign::of(path);
        if before == self.sign {
            return Look::Same;
        }
        let read = match path.map(std::fs::metadata) {
            Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => Err(GONE.to_string()),
            _ => settings_text(path)
                .and_then(|text| {
                    let read =
                        Instance::from_text(&text, path, self.given, self.all, home().as_deref())?;
                    Ok((read, text))
                })
                .map_err(|e| format!("{e:#}")),
        };
        if Sign::of(path) != before {
            return Look::Same;
        }
        self.sign = before;
        match read {
            Err(why) => Look::Refused(why),
            Ok((read, _)) if read == self.held => Look::Same,
            Ok((read, text)) => Look::Changed(read, text),
        }
    }

    /// Hold `instance`, a [`Look::Changed`]'s, once the caller has bound
    /// it (design D5).
    pub fn accept(&mut self, instance: Instance) {
        self.held = instance;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, UNIX_EPOCH};

    /// A scratch folder of the test's own, removed when dropped; no test
    /// reads or writes the host's own settings.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!(
                "sluice-host-settings-{name}-{}",
                std::process::id()
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

    fn changed(look: Look) -> Instance {
        match look {
            Look::Changed(i, _) => i,
            other => panic!("not changed: {other:?}"),
        }
    }

    fn refused(look: Look) -> String {
        match look {
            Look::Refused(why) => why,
            other => panic!("not refused: {other:?}"),
        }
    }

    /// The settings-take-up spec's "An unchanged file made unreadable": a
    /// look at a file whose sign stands reads nothing, so a file sluice
    /// could no longer read is no refusal while it stands (core.31).
    #[test]
    fn a_look_at_a_file_that_stands_reads_nothing() {
        let scratch = Scratch::new("stands");
        let path = scratch.0.join("config.toml");
        std::fs::write(&path, "[runs]\nmax = 2\n").unwrap();
        let mut settings = HostSettings::at(Some(&path), 16, 16).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let look = settings.look();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(look, Look::Same), "{look:?}");
        assert_eq!(settings.held().bound, 2);
    }

    /// The spec's "A file written after the run started" and "The same
    /// settings written again": a file that comes is read once, and the
    /// same settings written again change nothing.
    #[test]
    fn a_changed_file_is_read_once_and_the_same_settings_change_nothing() {
        let scratch = Scratch::new("once");
        let path = scratch.0.join("config.toml");
        let mut settings = HostSettings::at(Some(&path), 16, 16).unwrap();
        assert_eq!(settings.held().bound, 4);
        assert!(matches!(settings.look(), Look::Same));

        std::fs::write(&path, "[runs]\nmax = 3\n").unwrap();
        let three = changed(settings.look());
        assert_eq!(three.bound, 3);
        settings.accept(three);
        assert!(matches!(settings.look(), Look::Same));

        let was = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, "[runs]\nmax = 3\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(was + Duration::from_secs(1))
            .unwrap();
        let look = settings.look();
        assert!(matches!(look, Look::Same), "{look:?}");
        assert_eq!(settings.held().bound, 3);
    }

    /// The spec's "A link moved to a store file of the same size and time":
    /// the link's own entry and the path it resolves to see what the
    /// target's time and size cannot (design D1).
    #[test]
    fn a_link_moved_to_a_file_of_the_same_size_and_time_is_a_change() {
        let scratch = Scratch::new("link");
        let store = scratch.0.join("store");
        std::fs::create_dir_all(&store).unwrap();
        let (a, b) = (store.join("a-config.toml"), store.join("b-config.toml"));
        for (file, text) in [(&a, "[runs]\nmax = 2\n"), (&b, "[runs]\nmax = 3\n")] {
            std::fs::write(file, text).unwrap();
            std::fs::File::options()
                .write(true)
                .open(file)
                .unwrap()
                .set_modified(UNIX_EPOCH + Duration::from_secs(1))
                .unwrap();
        }
        let path = scratch.0.join("config.toml");
        std::os::unix::fs::symlink(&a, &path).unwrap();
        let mut settings = HostSettings::at(Some(&path), 16, 16).unwrap();
        assert_eq!(settings.held().bound, 2);

        let next = scratch.0.join("config.toml.next");
        std::os::unix::fs::symlink(&b, &next).unwrap();
        std::fs::rename(&next, &path).unwrap();
        assert_eq!(changed(settings.look()).bound, 3);
    }

    /// The spec's "A file sluice refuses, and then mends" and "A gone
    /// file": the settings held stand, said once for each change (design
    /// D4; core.54).
    #[test]
    fn a_file_that_does_not_read_leaves_the_settings_held_and_is_said_once() {
        let scratch = Scratch::new("refused");
        let path = scratch.0.join("config.toml");
        std::fs::write(&path, "[runs]\nmax = 2\n").unwrap();
        let mut settings = HostSettings::at(Some(&path), 16, 16).unwrap();

        std::fs::write(&path, "[runs]\nmax = \"two\"\n").unwrap();
        let why = refused(settings.look());
        assert!(why.contains("max"), "{why}");
        assert!(why.contains(&path.display().to_string()), "{why}");
        assert_eq!(settings.held().bound, 2);
        assert!(matches!(settings.look(), Look::Same));

        std::fs::write(&path, "[runs]\nmax = 3\n").unwrap();
        let three = changed(settings.look());
        assert_eq!(three.bound, 3);
        settings.accept(three);

        std::fs::remove_file(&path).unwrap();
        assert_eq!(refused(settings.look()), GONE);
        assert_eq!(settings.held().bound, 3);
        assert!(matches!(settings.look(), Look::Same));
    }

    /// The spec's "The first look after a hand-over": settings handed over
    /// are held under no sign, so the first look reads the file once and
    /// finds them, a change made after is taken up, and a file that does
    /// not read leaves what is held (135; record 0107; core.54; design D4).
    #[test]
    fn a_run_handed_over_holds_what_it_was_handed_and_reads_the_file_once() {
        let scratch = Scratch::new("handed");
        let path = scratch.0.join("config.toml");
        let text = "[runs]\nmax = 3\n";
        std::fs::write(&path, text).unwrap();
        let mut settings = HostSettings::handed(text, Some(&path), 16, 16).unwrap();
        assert_eq!(settings.held().bound, 3);

        let look = settings.look();
        assert!(matches!(look, Look::Same), "{look:?}");
        assert_eq!(settings.held().bound, 3);

        std::fs::write(&path, "[runs]\nmax = 4\n").unwrap();
        let four = match settings.look() {
            Look::Changed(i, read) => {
                assert_eq!(read, "[runs]\nmax = 4\n");
                i
            }
            other => panic!("not changed: {other:?}"),
        };
        assert_eq!(four.bound, 4);
        settings.accept(four);

        std::fs::write(&path, "[runs]\nmax = \"two\"\n").unwrap();
        let why = refused(settings.look());
        assert!(why.contains("max"), "{why}");
        assert_eq!(settings.held().bound, 4);
    }

    /// Design D3: the settings a host process reads are the file its own
    /// start environment names, `XDG_CONFIG_HOME`'s, never this process's.
    #[test]
    fn the_settings_are_read_from_the_environment_given() {
        let dir = Scratch::new("read-in");
        let path = dir.0.join("sluice/config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[runs]\nmax = 3\n").unwrap();
        let env = vec![(
            std::ffi::OsString::from("XDG_CONFIG_HOME"),
            dir.0.clone().into_os_string(),
        )];
        let settings = HostSettings::read_in(&env).unwrap();
        assert_eq!(settings.path(), Some(path.as_path()));
        assert_eq!(settings.held().bound, 3);
    }
}
