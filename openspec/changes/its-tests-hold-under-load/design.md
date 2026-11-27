## Context

Each cause is a race that a busy host makes likely. One step of sluice
reads, or another process acts, in a gap that is short on a quiet host.
The fix closes each gap. No fix waits longer or tries again.

## Decisions

### D1: The host's lock goes to the new image by its descriptor

`start_afresh` clears the lock's close-on-exec flag, names its
descriptor in `SLUICE_LOCK_FD`, and replaces its image while it holds
the lock. `take_carried` takes the descriptor first in `main`, sets it
to close on exec again, and `run` and `daemon` hold it as their lock.
The lock is the same open file in both images, so it is never free.
Where the start fails, the process keeps its lock as it was. A copy that
a forked child holds of the lock is then harmless: the new image needs
no new lock.

A sluice released before this change hands no lock. The image it starts
takes the lock as before.

### D2: A kept read of a request stands while its entry stands

`Seen` is the read that a follow keeps. It holds the request's entry as
the host's records held it at the read. The entry is the latest
submission of its place, by number and state. `follow` reads the entry first. It uses the kept
read only within a quarter of `discovery` and while the entry is the
same. Rule core.32 lets sluice read a request again once its entry
moved, so a wait reads no more often than the moves of its request.
Intake withdraws an entry when it reads its request closed: the wait
that follows it then reads GitHub, and GitHub's answer decides it.

### D3: Reports before documents

The subscription's loop waits in a biased `select!`: its token, the
client's line, a report, a document. The repository's task sends a
pass's report to its followers before it starts to read the document of
that pass. So when the loop finds both ready, the report goes first.

### D4: One write of a hooks folder at a time

`install_at`, `reinstall_at` and `remove` take one lock of the process
before they read the hook. Every write of a hook is in `sluice host`
(rule core.25), so a lock of the process orders them all. A name for
each temporary file would end the lost rename alone. It would not stop
two installs that both move the person's hook aside.

### D5: Tests that read what they mean

We fix a test that races only with itself in the test. The race of
`reader_tick.rs` exists only where the host passes only when asked. A
run on a host that passes at its own ticks reads its passes on its
subscription. The lines of the subscription come after `sluice host`
puts back the hook. The test
now waits for the watch's first document, which comes after the
finding. `an_upgrade_keeps_its_runs_as_children` reads the log at the
hand-over. The `states` helper of `instance.rs` fails with the
status's output.

## Risks

A follow whose request's entry moves often reads GitHub more often than
once a quarter of `discovery`. An entry moves at a submit, an intake, a
verdict or a withdrawal, which is not more often than the request's own
moves.
