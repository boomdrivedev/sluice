//! The session a submit runs in, as herdr names it, and the outcome
//! pushed into it (50, 51; C.5 worktrunk; ADR 0068). A pane herdr runs
//! has `HERDR_PANE_ID` and `HERDR_SOCKET_PATH`; the session is
//! `herdr:<pane>@<socket>`, which the core shows and never reads into
//! (rule core.1). The outcome is one line of sluice's words, typed into
//! the pane as a prompt to its agent by `herdr agent prompt`, whose
//! acceptance is the delivery.

use std::time::Duration;

use anyhow::Result;

/// How long herdr has to take a prompt before the push is given up.
pub const BOUND: Duration = Duration::from_secs(10);

pub(crate) const PANE: &str = "HERDR_PANE_ID";
pub(crate) const SOCKET: &str = "HERDR_SOCKET_PATH";
const PREFIX: &str = "herdr:";

/// 51: the session this process runs in, or `None` outside any herdr
/// pane.
pub fn session_here() -> Option<String> {
    session_of(crate::caller::var)
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
    let mut c = prompt_command(pane, socket, line, std::env::vars_os().map(|(k, _)| k));
    let out = match crate::workers::output_within(&mut c, bound, weir_bounded::GRACE) {
        Ok(out) => out,
        Err(e) => return Ok(Answer::Refused(format!("running herdr: {e}"))),
    };
    Ok(match out {
        Some(o) if o.status.success() => Answer::Took,
        Some(o) => Answer::Refused(refusal(&o.stderr)),
        None => Answer::Refused(format!("herdr did not answer within {}s", bound.as_secs())),
    })
}

/// The herdr command that types `line` into `pane` at `socket`: none of
/// `held`, the names of this process's own variables, that is herdr's
/// reaches it, so a round run from another pane reaches the recorded
/// server.
fn prompt_command(
    pane: &str,
    socket: &str,
    line: &str,
    held: impl Iterator<Item = std::ffi::OsString>,
) -> std::process::Command {
    let mut c = crate::workers::command("herdr", "notify");
    for k in held {
        if k.to_str().is_some_and(|k| k.starts_with("HERDR_")) {
            c.env_remove(&k);
        }
    }
    c.args(["agent", "prompt", pane, line]).env(SOCKET, socket);
    c
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
    /// 51: a prompt types its line into the recorded pane, on the
    /// recorded server, and carries none of this process's own herdr
    /// variables, so a round run from another pane reaches the pane the
    /// submit recorded (`a_round_in_another_pane_reaches_the_recorded_server`).
    #[test]
    fn a_prompt_carries_none_of_this_panes_variables() {
        let held = [
            "HERDR_PANE_ID",
            "HERDR_SOCKET_PATH",
            "HERDR_SESSION",
            "PATH",
        ]
        .into_iter()
        .map(std::ffi::OsString::from);
        let c = prompt_command("w1:p2", "/run/a.sock", "[sluice] #1 landed", held);
        let args: Vec<_> = c
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["agent", "prompt", "w1:p2", "[sluice] #1 landed"]);
        let envs: std::collections::BTreeMap<_, _> = c
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        assert_eq!(envs.get(SOCKET), Some(&Some("/run/a.sock".to_string())));
        assert_eq!(envs.get(PANE), Some(&None));
        assert_eq!(envs.get("HERDR_SESSION"), Some(&None));
        assert!(!envs.contains_key("PATH"), "{envs:?}");
    }

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
