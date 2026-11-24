# Proposal

## Why

On 2026-11-24 one host took sluice 0.105.1, and herdr's sluice plugin on that host still ran sluice 0.99.2. Each command of 0.105.1 asked the host process of 0.99.2 to hand over, and that process ended. The plugin's watch, a client of 0.99.2, then found no host process and started one of 0.99.2. The host started eight host processes in twenty seconds, and the host process of 0.105.1 never started.

The claim `a-newer-command-replaces-an-older-host-process@c8ef83408996` covers one way only: a newer command replaces an older host process. Nothing stops an older sluice from starting a host process again after a newer one has run. A host process does not record which sluice has run, so no sluice can know that it is older than the host's.

Claims that this change makes hold:

- `a-newer-command-replaces-an-older-host-process`, in a new version that amends `c8ef83408996`: a sluice older than the newest host process that has run on the host starts no host process, and says why.
- `never-break-an-existing-client@b539ef8e2413`: sluice has no public release yet, so a sluice older than the host's may be refused.

## What Changes

- Each host process records the newest sluice that has run as the host process of its state folder, in `host/newest`, as it starts. It raises the record and never lowers it.
- A command of a sluice older than that record starts no host process. Where no host process answers, it fails, and its error names its own version, the newer version, and the sluice to run.
- `sluice host` and `sluice daemon` of a sluice older than that record end as they start. They take no lock, ask no host process to hand over, and say why on their standard error.
- A command of a sluice older than that record asks no host process to hand over. It uses the host process that answers as it is. `sluice hand-over` of such a sluice fails and asks nothing.
- A host process refuses a hand-over that a sluice older than that record asks for. It answers with an error that names both versions, and goes on as it was. A newer command that gets this error fails at once with its words.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `host-process`: four requirements are added. A host process records the newest sluice that has run. An older sluice starts no host process and asks for no hand-over. A host process hands over to no older sluice.

## Impact

- `crates/sluice/src/host_process/mod.rs`: `host/newest`, its read and its write, the refusal of `sluice host` and `sluice daemon`, and the refusal of a hand-over.
- `crates/sluice/src/host_process/client.rs`: the refusal of a start, of a hand-over that the command asks, and of `sluice hand-over`.
- `crates/sluice/tests/host_process.rs`: a test for each scenario.
- The claim `a-newer-command-replaces-an-older-host-process`, a new version proposed, for the user to accept.
