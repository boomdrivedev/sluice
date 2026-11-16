//! A fake HTTP server on a port the system picks, answering each request
//! with what the test's handler says and keeping every request it saw.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// An answer: its status, its headers and its body.
pub type Answer = (u16, Vec<(String, String)>, String);

/// The status of an answer never sent: the connection is closed after the
/// request, as one cut is.
pub const CUT: u16 = 0;

pub struct Fake {
    url: String,
    seen: Arc<Mutex<Vec<Request>>>,
}

impl Fake {
    pub fn start(handler: impl Fn(&Request) -> Answer + Send + Sync + 'static) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        let handler = Arc::new(handler);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (kept, handler) = (kept.clone(), handler.clone());
                // A connection of its own each: a client may open one and
                // send nothing on it.
                std::thread::spawn(move || answer(stream, &kept, &*handler));
            }
        });
        Fake { url, seen }
    }

    /// `http://127.0.0.1:<port>`
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn requests(&self) -> Vec<Request> {
        self.seen.lock().unwrap().clone()
    }
}

/// A remote that takes every connection, reads its request and never
/// answers it, as a server stalled mid-call does: each connection is held
/// open from its side, so a test can tell whether the client let it go.
pub struct Silent {
    url: String,
    seen: Arc<Mutex<Vec<Request>>>,
    held: Arc<Mutex<Vec<TcpStream>>>,
}

impl Silent {
    pub fn start() -> Silent {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (seen, held) = (
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(Vec::new())),
        );
        let (s, h) = (seen.clone(), held.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (s, h) = (s.clone(), h.clone());
                std::thread::spawn(move || {
                    if let Some(req) = read(&mut BufReader::new(&stream)) {
                        s.lock().unwrap().push(req);
                    }
                    h.lock().unwrap().push(stream);
                });
            }
        });
        Silent { url, seen, held }
    }

    /// `http://127.0.0.1:<port>`
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn requests(&self) -> Vec<Request> {
        self.seen.lock().unwrap().clone()
    }

    /// Whether the client has closed every connection it opened, within
    /// a moment for the system to tell.
    pub fn all_let_go(&self) -> bool {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let open = self.held.lock().unwrap().iter().any(|stream| {
                stream.set_nonblocking(true).unwrap();
                let mut b = [0u8; 512];
                loop {
                    match (&*stream).read(&mut b) {
                        Ok(0) => break false,
                        Ok(_) => continue,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break true,
                        Err(_) => break false,
                    }
                }
            });
            if !open || std::time::Instant::now() >= until {
                return !open;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

/// Read one request from `stream`, keep it in `kept`, and write
/// `handler`'s answer, or cut the connection.
fn answer(mut stream: TcpStream, kept: &Mutex<Vec<Request>>, handler: &dyn Fn(&Request) -> Answer) {
    let Some(req) = read(&mut BufReader::new(&stream)) else {
        return;
    };
    kept.lock().unwrap().push(req.clone());
    let (status, headers, body) = handler(&req);
    if status == CUT {
        return;
    }
    // A body no string holds is sent as the bytes its base64 says.
    let binary = headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(BYTES));
    let body = match binary {
        true => {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(body.trim())
                .expect("a body of base64")
        }
        false => body.into_bytes(),
    };
    let mut out = format!("HTTP/1.1 {status} X\r\nConnection: close\r\n");
    for (k, v) in headers
        .iter()
        .filter(|(k, _)| !k.eq_ignore_ascii_case(BYTES))
    {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let mut out = out.into_bytes();
    out.extend(body);
    let _ = stream.write_all(&out);
}

/// The header an answer names to have its body, base64, sent as the bytes
/// it says: one no string holds, as an object S3 keeps.
pub const BYTES: &str = "x-fake-body-base64";

/// An answer of `bytes`, as `content_type`.
#[allow(dead_code)]
pub fn bytes(status: u16, content_type: &str, bytes: &[u8]) -> Answer {
    use base64::Engine as _;
    (
        status,
        vec![
            ("Content-Type".into(), content_type.into()),
            (BYTES.into(), "1".into()),
        ],
        base64::engine::general_purpose::STANDARD.encode(bytes),
    )
}

fn read(r: &mut impl BufRead) -> Option<Request> {
    let mut line = String::new();
    r.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        r.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':')?;
        headers.push((k.trim().to_string(), v.trim().to_string()));
    }
    let len = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; len];
    r.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into(),
    })
}

/// A JSON answer.
pub fn json(status: u16, body: serde_json::Value) -> Answer {
    (
        status,
        vec![("Content-Type".into(), "application/json".into())],
        body.to_string(),
    )
}

/// How many times a handler took it, from 0: for an answer that changes
/// after the first few requests.
#[derive(Clone, Default)]
pub struct Count(Arc<AtomicUsize>);

impl Count {
    /// The count before this take.
    pub fn take(&self) -> usize {
        self.0.fetch_add(1, Ordering::SeqCst)
    }

    pub fn get(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

/// A server unable to answer just then: GitHub's 503.
pub fn unavailable() -> Answer {
    (503, vec![], "Service Unavailable\n".into())
}

// git's smart HTTP, as far as a fake remote speaks it: its listing of
// refs, a push's commands and its report.

/// A repository with nothing in it, as git's smart HTTP lists one.
pub fn empty_repository() -> Answer {
    listing("git-upload-pack", &[])
}

/// `s` as a pkt-line.
pub fn pkt(s: &str) -> String {
    format!("{:04x}{s}", s.len() + 4)
}

/// The fake's refs, each a ref and its revision, as git's smart HTTP lists
/// them for `service`: `git-upload-pack` for a fetch, `git-receive-pack`
/// for a push.
pub fn listing(service: &str, refs: &[(&str, &str)]) -> Answer {
    let caps = match service {
        "git-receive-pack" => "report-status delete-refs side-band-64k ofs-delta",
        _ => "multi_ack ofs-delta",
    };
    let zero = "0".repeat(40);
    let mut lines = match refs {
        [] => vec![format!("{zero} capabilities^{{}}")],
        _ => refs.iter().map(|(r, at)| format!("{at} {r}")).collect(),
    };
    lines[0] = format!("{}\0{caps}", lines[0]);
    let body = format!(
        "{}0000{}0000",
        pkt(&format!("# service={service}\n")),
        lines
            .iter()
            .map(|l| pkt(&format!("{l}\n")))
            .collect::<String>()
    );
    (
        200,
        vec![(
            "Content-Type".into(),
            format!("application/x-{service}-advertisement"),
        )],
        body,
    )
}

/// A push's commands, `(old, new, ref)` each, as git sends them at the
/// head of a `POST /git-receive-pack`.
pub fn commands(r: &Request) -> Vec<(String, String, String)> {
    let body = r.body.as_bytes();
    let mut at = 0;
    let mut all = Vec::new();
    while let Some(len) = body
        .get(at..at + 4)
        .and_then(|h| usize::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok())
    {
        if len < 4 {
            break;
        }
        let line = String::from_utf8_lossy(&body[at + 4..at + len]);
        let line = line.split('\0').next().unwrap().trim_end();
        let mut words = line.split(' ').map(String::from);
        if let (Some(old), Some(new), Some(name)) = (words.next(), words.next(), words.next()) {
            all.push((old, new, name));
        }
        at += len;
    }
    all
}

/// The fake's answer to a push it takes: every ref it names updated.
pub fn received(r: &Request) -> Answer {
    let ok: Vec<String> = commands(r)
        .iter()
        .map(|(_, _, name)| format!("ok {name}"))
        .collect();
    report(&ok, None)
}

/// The fake's report of a push: `unpack ok`, then `lines`, one for each
/// ref, on side band 1, after `said` on side band 2, which git prints on
/// stderr as the remote's.
pub fn report(lines: &[String], said: Option<&str>) -> Answer {
    let status: String = std::iter::once("unpack ok")
        .chain(lines.iter().map(String::as_str))
        .map(|l| pkt(&format!("{l}\n")))
        .collect();
    let said = said.map_or(String::new(), |s| pkt(&format!("\x02{s}\n")));
    (
        200,
        vec![(
            "Content-Type".into(),
            "application/x-git-receive-pack-result".into(),
        )],
        format!("{said}{}0000", pkt(&format!("\x01{status}0000"))),
    )
}

/// Whether `r` lists the fake's refs for a push.
pub fn lists_for_push(r: &Request) -> bool {
    r.method == "GET" && r.path.ends_with("?service=git-receive-pack")
}

/// Whether `r` is a push: its commands and its objects.
pub fn is_push(r: &Request) -> bool {
    r.method == "POST" && r.path.ends_with("/git-receive-pack")
}

/// The answer of a fake remote holding `refs` and taking every push.
pub fn takes(r: &Request, refs: &[(&str, &str)]) -> Answer {
    if lists_for_push(r) {
        listing("git-receive-pack", refs)
    } else if is_push(r) {
        received(r)
    } else {
        listing("git-upload-pack", refs)
    }
}
