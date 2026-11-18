//! 153: the hook that links a span testing a combination to the trace each
//! of its submissions kept (design D5 of
//! `a-sluice-command-shows-inside-its-callers-trace`; rule core.53). The
//! core names no backend (record 0014), so it hands each kept context, as
//! the text its record keeps, to a function the binary sets once as its
//! telemetry starts sending traces. Until one is set it does nothing.

use std::sync::OnceLock;

/// What links `span` to the trace a submission kept: the span, the
/// `TRACEPARENT` its record keeps, and the submission's number.
pub type Link = fn(&tracing::Span, &str, u64);

static LINK: OnceLock<Link> = OnceLock::new();

/// Set the function every link is made by, once for the process; a later
/// set changes nothing.
pub fn set(link: Link) {
    let _ = LINK.set(link);
}

/// Link `span` to `trace`, the context `submission` was submitted under,
/// where a function is set; nothing otherwise.
pub fn link(span: &tracing::Span, trace: &str, submission: u64) {
    if let Some(link) = LINK.get() {
        link(span, trace, submission);
    }
}
