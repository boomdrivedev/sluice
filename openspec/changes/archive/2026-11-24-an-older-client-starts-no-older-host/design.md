# Design

## Context

A command reaches the host process through its socket and says its version in `hello` (`crates/sluice/src/host_process/client.rs`, `Client::answering`). Where the host process is older, the command asks it to hand over (`Client::afresh`). Where none answers, the command takes `host/start.lock` and starts `sluice host` from its own executable (`Client::start`). A daemon that starts asks each host process that is no daemon to hand over and end, whatever its version (`take_place` and `handed_over` in `crates/sluice/src/host_process/mod.rs`).

A host process of sluice 0.99.2 ends when a newer command asks it to hand over. A client of 0.99.2 that finds no host process then starts one of 0.99.2. On 2026-11-24, herdr's sluice plugin ran 0.99.2 while the commands ran 0.105.1. Each command of 0.105.1 ended a host process of 0.99.2, and the plugin's watch started the next one. The host had eight host processes in twenty seconds, and none of 0.105.1.

No file of the host says which sluice has run as its host process. `host/version` names the one that runs now, and it changes back to the older version each time an older one starts.

## Goals / Non-Goals

**Goals:**

- A sluice older than the newest host process that has run on the host starts no host process, and says why, naming the newer version. (`a-newer-command-replaces-an-older-host-process`, the new version)
- No host process hands over to a sluice older than that, so an older daemon or an older `sluice hand-over` cannot put an older sluice in its place.

**Non-Goals:**

- A change to a released sluice. Sluice 0.99.2 reads no `host/newest`, and a client of it still starts a host process of its own. The host's plugin comes from the same flake as its sluice, so the plugin and the commands run one release once the host takes both (`plugins-come-from-the-tools-flake@78305fd0bae0`).
- A command to go back to an older sluice. The error says which file to remove for that.

## Decisions

### D1. Each host process records the newest sluice that has run, in `host/newest`

As it starts, after it takes the host's lock and before its socket answers, each host process reads `host/newest`. Where the file names no version, or names a version older than its own, the host process writes the file whole, through the folder that it holds (record 0093, decision 25): its own version and its executable.

```json
{"version":"0.105.1","program":"/nix/store/…-sluice-0.105.1/bin/sluice"}
```

It never lowers the record. A version that is no release number, as `older` reads it (`crates/sluice/src/host_process/version.rs`), is never older and never newer. So a build that is no release writes the file only where it names nothing, and no release is refused for it. A build of main says `0.0.0` (rule core.55), so a build of main is older than each release that has run.

Before: only `host/version` named a sluice, the one that runs now. Now: `host/newest` also names the newest that has run, and stays when an older one runs.

Not taken: the record written by a newer command before it asks for a hand-over. Today every host process starts afresh in place, with its socket, so no command finds the socket empty while it hands over. The newer host process writes the record as it starts afresh.

Not taken: `host/version` read as the record. It names the process that runs now, which an older sluice can be.

### D2. A sluice older than the record starts no host process

Each way that starts a host process reads `host/newest` first:

- A command that finds no host process answering, under the start lock (`Client::start`).
- `sluice host`, as it starts, before it takes the host's lock.
- `sluice daemon`, as it starts, before it takes the host's lock or asks a host process to hand over.

Where its own version is older than the record's, it starts nothing and fails with exit 3. The words name the two versions, the program that the record names, the sluice to run, and the file to remove to run the older one on purpose:

```text
sluice 0.99.2 starts no host process: sluice 0.105.1 has run as this host's process (/nix/store/…/bin/sluice). Run sluice 0.105.1 or newer, or remove /Users/me/.local/state/sluice/host/newest to run sluice 0.99.2's host process.
```

An image that starts afresh in place does not read the record. A host process asked for it (D3), and it keeps the runs of that process.

Not taken: an older command that starts a host process from the executable that the record names. The record's program can be gone, and a command of one release would then speak to a host process of another release that it did not ask for.

### D3. A host process hands over to no sluice older than the record

A command asks for a hand-over only where the host process is older than it, and only where its own version is not older than the record. A command that is older than the record uses the host process that answers as it is, as an older command uses a newer one. `sluice hand-over` of a sluice older than the record fails with the words of D2, and asks nothing.

The host process also refuses a hand-over itself. Where the `hello` of the connection that asks names a version older than the record, or older than its own, it answers with an error, says so in its log, and goes on as it was:

```text
sluice 0.99.2 asked this host process, sluice 0.105.1, to hand over, and a host process hands over to no sluice older than 0.105.1, which has run as this host's process. Run sluice 0.105.1 or newer.
```

This holds for a daemon's request to end as well. So an older daemon that a service manager starts cannot end a newer host process.

A command that asked for a hand-over reads the answer. Where it is an error, the command fails at once with those words. Before, it waited for a new host process for the whole wait of a hand-over.

Not taken: a newer command that refuses a host process that is older than it after a hand-over. A test client says a newer version than its executable, and the image that starts afresh is that executable.

## Risks / Trade-offs

- [A person who must run an older sluice, to go back from a release with a fault, is refused] → The error names `host/newest`. Removing it lets the older sluice start. The next newer host process writes it again.
- [A build of main, `0.0.0`, starts no host process in a state folder where a release has run] → Tests and builds of main use state folders of their own. A person who runs a build of main against the host's state folder gets the words of D2.
- [A client of a released sluice, 0.99.2 and older, does not read `host/newest`] → It still starts an older host process. Each newer command then asks that process to hand over again. This change cannot alter a released sluice. The fix is that the host runs one release of the plugin and the commands.
