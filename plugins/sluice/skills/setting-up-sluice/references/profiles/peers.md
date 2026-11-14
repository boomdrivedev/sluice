# Many hosts: signers and systems

**Choose it when** sessions on more than one host submit to the same branch,
when an agent should be able to submit and switch its host off, or when a
check must pass on more than one system (Linux and macOS, x86 and ARM).
Every host runs the same sluice; the queue lives on the git remote, and
there is no server.

## What turns it on

A branch with a `remote`, in a policy that has a `[signers]` table, keeps its
queue on that remote under `refs/sluice/`. Without `[signers]`, every branch
keeps its queue on the host as before, and `trust` keeps listing executor
kinds. Add signers in one reviewed landing that also rewrites `trust`.

## Keys

```toml
[systems]
linux-x64   = { os = "linux", arch = "x86_64" }
macos-arm64 = { os = "macos", arch = "aarch64" }

[signers.box]                    # a standing executor of the operator's
ssh = "exec@build-1 ssh-ed25519 AAAA..."
confined = true                  # its checkouts are containers no session reaches
systems = ["linux-x64"]

[signers.mac]                    # a Mac standing executor
ssh = "exec@mac-1 ssh-ed25519 AAAA..."
confined = true
systems = ["macos-arm64"]

[signers.sessions]               # the hosts agents work on
ssh = ".config/sluice.sessions"  # one allowed-signers line per host
confined = false

[branches."feature/*"]           # a placeholder: the project's own pattern
remote = "origin"
required = ["#check-fast"]
trust = ["sessions", "box", "mac"]   # no systems: a pass from any one

[branches.main]
remote = "origin"
required = ["#check-fast", "cli:build"]
trust = ["box", "mac"]
confined = true
systems = ["linux-x64"]

[branches.main.systems_for]
"cli:build" = ["linux-x64", "macos-arm64"]
```

- `trust` lists signer names. A result counts only when a signer the branch
  trusts signed it. A branch with a remote and no `trust` trusts no signer.
- `confined = true` counts only results whose signer has `confined = true`.
  Say it of a signer only when no session can reach its checkouts: a box
  that runs no sessions.
- `systems` is what every required check must pass on; `systems_for` names
  other systems for one check. A check with neither takes one pass from any
  system. Each system is one more run per combination.
- A replay from the task runner's cache counts only when a trusted signer
  recorded a pass for that key on that system. `cache` in `trust` is
  ignored, with a warning.
- A run waits for a host of its system whose signer the branch trusts, up to
  the branch's `timeout`. GitHub Actions as a pool is designed, not built.

## The host's own settings

In each host's own file, `~/.config/sluice/config.toml`, or where that host's
configuration is managed; check which on each host first (the skill's step 6,
*The host's file*):

```toml
[signing]
signer = "box"                     # the policy's name for this host's key
key = "~/.config/sluice/signing"   # an SSH private key, mode 0600

[runs]
systems = ["linux-x64"]            # default: the one system the host is

[peers]
lease = "5m"   # how long a quiet lead keeps a branch before another takes it
alive = "2m"   # how often this host shows it is alive; a claim lapses after it
name = "build-1" # default: the host name; set it when two hosts share one
```

sluice's host process on a host with `[signing]` claims the runs its
signer counts for, while it runs. A host with no `[signing]` records nothing on a branch kept on the
remote, and says so.

## Set it up, in order

Each step needs the one before it. Do the host steps on every host that
will submit or run checks.

1. **On each host, make its signing key.** sluice signs with no
   passphrase to type and refuses a key whose mode is not `0600`:

   ```sh
   ssh-keygen -t ed25519 -N "" -C "sluice $(hostname -s)" -f ~/.config/sluice/signing
   chmod 600 ~/.config/sluice/signing
   ```

2. **On each host, name its signer and key** in its own file (`[signing]`,
   above), or, where a tool manages the host's configuration, have the user
   add it there. Keep the key itself out of any store everyone may read. The signer is the name the policy will give it:
   one per kind of host, such as `box` for hosts that run no agents and
   `sessions` for the hosts agents work on. Set `[peers] name` when two
   hosts share a host name. On a host that runs only some of the policy's
   systems, set `[runs] systems`; sluice refuses a system the host is not.

3. **Collect each public half as an allowed-signers line**, the principal
   first (any label; sluice shows it, never checks it):

   ```sh
   echo "$(hostname -s) $(cat ~/.config/sluice/signing.pub)"
   ```

   One host per signer goes inline (`ssh = "<line>"`). Several hosts of one
   signer, usually the session hosts, go one line each in a file in the
   repository, named by `ssh = ".config/sluice.sessions"`. Adding a host
   later is a landing that adds its line.

4. **Land the policy in one change**: `[systems]` if checks need more than
   one, `[signers]`, and every branch with a remote rewritten from
   executor kinds to signer names in `trust`, with `confined = true` where
   it said `trust = ["confined"]`. Drop `cache` from `trust`. Land it
   through sluice as any change; the gate that tests it is the branch's
   current one. The first round after it lands moves each such branch's
   queue to the remote, and work queued on other hosts is submitted again
   under new ids (`sluice status <old id>` names the new one).

5. **Start the standing executors.** On each host that should take runs
   without a session waiting, keep a clone of the repository, and have a
   command of sluice's run in it whenever the host should take work:
   `sluice status` from whatever wakes the host, or the wake the
   deployment names. That command starts sluice's host process, which
   nothing installs: it takes the runs of its systems that its signer
   counts for, earliest first, up to `[runs] max`, and ends once none is
   left for it.

6. **Check it.** `sluice overview` lists each host serving the remote, with
   its signer, system, runs held and room. `sluice status <branch>` warns
   of anything in the policy it ignored. A session host with no
   `[signing]` says on every run that it records nothing. Then submit a
   harmless change with `--no-wait`, close the session, and see
   `sluice status <id>` read `landed` from another host.

What can go wrong:

- **Nothing runs a check on one system.** No trusted signer may claim it,
  or no host of it is up; the run waits until the branch's `timeout` and
  then fails naming the check and system.
- **A host's results never count.** Its signer is not in the branch's
  `trust`, or its key's line is not in the policy on the branch's current
  revision, or the branch has `confined = true` and its signer does not.
- **Two hosts both think they lead.** They share a host name: set
  `[peers] name` on one.
