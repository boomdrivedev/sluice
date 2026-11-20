//! 154: the hook a submission's life is sent by, once the write that ends
//! it is written (design D4 of `each-submission-shows-its-wait-and-its-test`;
//! rules core.53, core.66). The core names no backend (record 0014), so it
//! hands each life, as the facts it is sent with, to a function the binary
//! sets once as its telemetry starts sending traces, as it hands a kept
//! context to [`crate::links`]. Until one is set it does nothing.

use std::sync::OnceLock;

/// The facts a submission's life is sent with: from its submission to the
/// write that ended it.
#[derive(Clone, Debug, PartialEq)]
pub struct Life {
    /// The repository, as the host's binding names it (rule core.66).
    pub repository: String,
    pub branch: String,
    /// Its number.
    pub submission: u64,
    /// The head it submitted.
    pub head: String,
    /// The work it was submitted from: a worktree's branch or a request's
    /// `pull/<n>`.
    pub work: String,
    /// The agent that submitted it.
    pub agent: String,
    /// The `TRACEPARENT` its submit was started under, where it kept one
    /// (153).
    pub trace: Option<String>,
    /// When it was submitted, RFC 3339.
    pub submitted: String,
    /// The time of the write that ended it, RFC 3339.
    pub ended: String,
    /// How it ended, its state's own word: `landed`, `refused`, `failed`,
    /// `conflict` or `withdrawn`.
    pub outcome: &'static str,
    /// Its seconds waiting with no combination of it under test.
    pub waited_seconds: f64,
    /// Its seconds under test.
    pub tested_seconds: f64,
    /// The runs of its combinations that decided nothing (114).
    pub no_outcome: u32,
}

/// What sends a life.
pub type Send = fn(&Life);

static SEND: OnceLock<Send> = OnceLock::new();

/// Set the function every life is sent by, once for the process; a later
/// set changes nothing.
pub fn set(send: Send) {
    let _ = SEND.set(send);
}

/// Send `life` where a function is set; nothing otherwise.
pub fn ended(life: &Life) {
    if let Some(send) = SEND.get() {
        send(life);
    }
}
