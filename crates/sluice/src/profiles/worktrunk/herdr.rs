//! The session a submit runs in, as herdr names it, and the outcome
//! pushed into it (50, 51; C.5 worktrunk; ADR 0068). A pane herdr runs
//! has `HERDR_PANE_ID` and `HERDR_SOCKET_PATH`; the session is
//! `herdr:<pane>@<socket>`, which the core shows and never reads into
//! (rule core.1). The outcome is one line of sluice's words, typed into
//! the pane as a prompt to its agent by `herdr agent prompt`, whose
//! acceptance is the delivery.

use std::io::Read as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};

/// How long herdr has to take a prompt before the push is given up.
pub const BOUND: Duration = Duration::from_secs(10);

pub(crate) const PANE: &str = "HERDR_PANE_ID";
pub(crate) const SOCKET: &str = "HERDR_SOCKET_PATH";
const PREFIX: &str = "herdr:";

/// 51: the session this process runs in, or `None` outside any herdr
/// pane.
pub fn session_here() -> Option<String> {
    session_of(|k| std::env::var(k).ok())
}

/// The session the variables `var` reads name: `herdr:<pane>@<socket>`,
/// or `None` where either is unset or empty.
pub fn session_of(var: impl Fn(&str) -> Option<String>) -> Option<String> {
    let given = |k| var(k).filter(|v| !v.is_empty());
    Some(format!("{PREFIX}{}@{}", given(PANE)?, given(SOCKET)?))
}

/// A session this profile named: its pane and the socket of the herdr
/// server the pane is on. A pane's ID holds no `@`.
pub fn pane_of(session: &str) -> Option<(&str, &str)> {
    let (pane, socket) = session.strip_prefix(PREFIX)?.split_once('@')?;
    (!pane.is_empty() && !socket.is_empty()).then_some((pane, socket))
}

/// How herdr answered a prompt.
#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Took,
    /// herdr's words: the `error.message` of its JSON, or what it wrote.
    Refused(String),
}

/// Type `line` into `pane` of the herdr server at `socket`, as a prompt to
/// its agent. None of this process's own herdr variables reach herdr, so
/// a round run from another pane reaches the recorded server; one that
/// does not answer within `bound` is ended.
pub fn prompt(pane: &str, socket: &str, line: &str, bound: Duration) -> Result<Answer> {
    let mut c = Command::new("herdr");
    for (k, _) in std::env::vars_os() {
        if k.to_str().is_some_and(|k| k.starts_with("HERDR_")) {
            c.env_remove(&k);
        }
    }
    let child = c
        .args(["agent", "prompt", pane, line])
        .env(SOCKET, socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Ok(Answer::Refused(format!("running herdr: {e}"))),
    };
    let mut stderr = child.stderr.take().context("herdr's stderr")?;
    let said = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = stderr.read_to_end(&mut v);
        v
    });
    let until = Instant::now() + bound;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break Some(s);
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let said = said.join().unwrap_or_default();
    Ok(match status {
        Some(s) if s.success() => Answer::Took,
        Some(_) => Answer::Refused(refusal(&said)),
        None => Answer::Refused(format!("herdr did not answer within {}s", bound.as_secs())),
    })
}

/// herdr's words for a refusal: the message of the JSON error it writes,
/// or its stderr as written when that is not one.
fn refusal(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr).trim().to_string();
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 51: a pane is named by its ID and its server's socket, and only
    /// where both are set.
    #[test]
    fn a_session_is_its_pane_on_its_server() {
        let env = |pane: Option<&str>, socket: Option<&str>| {
            session_of(|k| match k {
                PANE => pane.map(str::to_string),
                SOCKET => socket.map(str::to_string),
                _ => None,
            })
        };
        let named = env(Some("w1:p2"), Some("/run/herdr.sock"));
        assert_eq!(named.as_deref(), Some("herdr:w1:p2@/run/herdr.sock"));
        assert_eq!(
            pane_of(named.as_deref().unwrap()),
            Some(("w1:p2", "/run/herdr.sock"))
        );
        assert_eq!(
            pane_of("herdr:w1:p2@/home/a@b/herdr.sock"),
            Some(("w1:p2", "/home/a@b/herdr.sock"))
        );
        assert_eq!(env(Some("w1:p2"), None), None);
        assert_eq!(env(None, Some("/run/herdr.sock")), None);
        assert_eq!(env(Some(""), Some("/run/herdr.sock")), None);
        assert_eq!(env(Some("w1:p2"), Some("")), None);
        assert_eq!(pane_of("tmux:%1"), None);
        assert_eq!(pane_of("herdr:@/run/herdr.sock"), None);
    }

    /// herdr's refusal in its own words: its JSON error's message, or
    /// what it wrote.
    #[test]
    fn a_refusal_is_in_herdrs_words() {
        assert_eq!(
            refusal(br#"{"error":{"code":"agent_blocked","message":"agent is blocked"}}"#),
            "agent is blocked"
        );
        assert_eq!(refusal(b"no such pane\n"), "no such pane");
    }
}
