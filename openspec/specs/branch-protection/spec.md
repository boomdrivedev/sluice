# branch-protection Specification

## Purpose
What the branch protection `sluice protect` installs refuses and lets
through on a host, judged by each branch's own value there, and what a
refusal tells whoever ran the command and the branch's records, in the
repository git names wherever the command runs (1, 124).

## Requirements

### Requirement: A write is judged by the branch's own value on the host
The protection SHALL judge a write to a branch it protects by the
value that branch's ref holds on the host before the write and the
value it would hold after. On a branch whose policy names a remote, it
SHALL fetch nothing and SHALL NOT compare the write with the remote's
revision. A write that leaves every protected branch at the value it
held SHALL pass; one that changes a protected branch's value, or
deletes it while anything still needs it, SHALL be refused (1, 124).

#### Scenario: A reset to where the copy stands passes
- **WHEN** `bolt/x` names `remote = "origin"`, the host's `bolt/x` is at `2111d21` while origin's is at `8079db3`, and someone runs `git reset --hard` in a worktree that has checked out `bolt/x`
- **THEN** git's reset succeeds, the host's `bolt/x` is still `2111d21`, and the branch's log has no `protected` line

#### Scenario: git's packing passes on a copy apart from the remote
- **WHEN** the host's `bolt/x` is at `2111d21`, origin's is at `8079db3`, and `git pack-refs --all --prune` runs
- **THEN** it succeeds, `bolt/x` is packed at `2111d21`, its loose file is gone, and the branch's log has no `protected` line

#### Scenario: A move of the copy to the remote's revision is refused
- **WHEN** the host's `bolt/x` is at `10e4791`, origin's is at `8079db3`, and someone runs `git update-ref refs/heads/bolt/x 8079db3`
- **THEN** git fails, the host's `bolt/x` is still `10e4791`, and the branch's log has one `protected` line from `10e4791` to `8079db3` naming the `git update-ref` command

#### Scenario: A branch with no remote is judged the same way
- **WHEN** `bolt/x` has no `remote` and someone runs `git reset --hard` in a worktree on it, then `git reset --hard unit/a` there
- **THEN** the first passes, the second is refused, and `bolt/x` is where it was

### Requirement: The protection judges the repository git names
The protection SHALL judge a transaction in the repository it is of, as
git names it, wherever the command was run from: the repository
`GIT_DIR` names when git sets it, and otherwise the one holding the
folder the command ran in. It SHALL refuse nothing of a ref it does not
guard. A transaction it cannot read for a reason of its own SHALL pass,
with one warning saying why, rather than aborting git's write (124).

#### Scenario: A fetch into a bare repository run from outside it
- **WHEN** sluice's protection is installed in the bare repository `x.git`, whose `bolt/x` sluice lands on, and from a folder that is no repository someone runs `git --git-dir=<path to x.git> fetch origin` that updates a remote-tracking ref and a branch sluice does not guard
- **THEN** the fetch succeeds, every ref it fetched is updated in `x.git`, sluice prints no warning, and `bolt/x`'s log has no `protected` line

#### Scenario: A write to a protected branch through `--git-dir` from outside is refused
- **WHEN** from that same outside folder someone runs `git --git-dir=<path to x.git> update-ref refs/heads/bolt/x <another revision>`
- **THEN** git fails, `x.git`'s `bolt/x` is where it was, and `bolt/x`'s log in `x.git` has one `protected` line naming the command

#### Scenario: Run from inside another repository, the named one is judged
- **WHEN** a second repository `y`, whose policy protects `main`, holds the folder the command runs in, and from there someone runs `git --git-dir=<path to x.git> update-ref refs/heads/main <another revision>`, where `x.git`'s policy guards no `main`
- **THEN** the update succeeds in `x.git`, `y`'s `main` is untouched, and neither repository's log has a `protected` line

#### Scenario: A transaction the protection cannot read passes, said once
- **WHEN** the policy at `bolt/x`'s revision and at `bolt/y`'s cannot be parsed, and someone runs `git update-ref --stdin` with one transaction moving both
- **THEN** the update succeeds, git's error output holds exactly one warning from sluice saying its branch protection let the transaction through and why, and no log has a `protected` line

### Requirement: A refusal is recorded with the command and both revisions
A refused write whose cause does not stand SHALL be recorded in the
branch's log as `protected`, with the branch's ref, the value it held on
the host, the value the write would have set or none for a deletion,
the command that tried it as git ran it, and the host. A refused write
whose cause stands SHALL add no line and SHALL be counted with its cause
in the branch's state, as refused writes are kept (1, 124, 148).

#### Scenario: The log line of a refused move
- **WHEN** the host's `bolt/x` is at `10e4791` and `git branch -f bolt/x unit/a` is refused
- **THEN** the branch's log has a `protected` line with `ref` `refs/heads/bolt/x`, `from` `10e4791`, `to` `unit/a`'s revision, `by` naming `git branch -f bolt/x unit/a`, and `host` the host's name

#### Scenario: The same command refused again
- **WHEN** `git -C <repository> update-ref refs/heads/bolt/x <new> <old>` is refused, and then again with two other full revisions
- **THEN** the branch's log has one `protected` line, for the first, and `branch.json`'s `refused_writes` has one cause for it with `times` 2 (`a_timers_writes_are_one_cause` in `crates/sluice/tests/protect.rs`)

### Requirement: A refusal tells whoever ran it in sluice's words
Whoever ran a refused command SHALL be told, before git's own error,
that the branch is protected by sluice, the command that tried, the
revision the branch holds on the host and the one the write would have
set, and how to bring the worktree level again. On a branch whose
policy names a remote, it SHALL also say that sluice levels the host's
copy of the branch itself, at each landing (1, 124).

#### Scenario: The refusal names the command and both revisions
- **WHEN** the host's `bolt/x` is at `10e4791`, origin's at `8079db3`, and a tool runs `git update-ref refs/heads/bolt/x 8079db3`
- **THEN** git's error output starts with sluice's lines saying `bolt/x is protected by sluice`, naming `git update-ref refs/heads/bolt/x 8079db3`, `10e4791` and `8079db3`, and saying sluice levels this host's `bolt/x` at each landing; git's own `fatal` line comes after them

#### Scenario: A deletion's refusal says what still needs the branch
- **WHEN** `bolt/x` holds submission #1 in its queue and someone runs `git branch -D bolt/x`
- **THEN** git fails, sluice's lines name the command, the revision `bolt/x` holds, and #1, and the branch's log has a `protected` line with no `to`

### Requirement: The protection reads only what guarding needs
Of the host's settings, the protection SHALL read only `[forge]`, how
sluice reaches a branch's remote. Of the policy it SHALL read only the
`[branches]` patterns, each one's `remote`, and whether `[signers]`
names a signer: which branches are guarded and where their records are.
It SHALL pass over every other table and field, known to it or not, so
a host whose settings a newer sluice wrote is guarded by an older hook
as before.

#### Scenario: A host setting the hook does not know changes nothing it guards
- **WHEN** the host's `~/.config/sluice/config.toml` holds `[runs] cores = 12` and a table `[later]` with `x = 1`, which no sluice yet reads, and someone runs `git update-ref refs/heads/bolt/x <unit/a's revision>`
- **THEN** git fails with sluice's refusal naming the command, `bolt/x` is where it was, its log has one `protected` line, and sluice prints no warning about the settings

#### Scenario: A move that changes nothing still passes under such settings
- **WHEN** the host's settings hold that same unknown table and someone runs `git reset --hard` in a worktree on `bolt/x`
- **THEN** the reset succeeds, `bolt/x` is where it was, and sluice prints nothing

#### Scenario: A policy field the hook does not know changes nothing it guards
- **WHEN** the policy at `bolt/x`'s revision gives `[branches."bolt/*"]` a field `later = true` and has a table `[later]`, neither of which any sluice yet reads, and someone runs `git branch -f bolt/x unit/a`
- **THEN** git fails with sluice's refusal, `bolt/x` is where it was, and its log has one `protected` line

#### Scenario: A branch on a remote is still judged as sluice's copy
- **WHEN** `bolt/x` names `remote = "origin"`, the policy and the host's settings each hold a field no sluice yet reads, the host's `bolt/x` is at `10e4791` and origin's at `8079db3`, and someone runs `git update-ref refs/heads/bolt/x 8079db3`
- **THEN** git fails, the refusal says sluice levels this host's `bolt/x` at each landing, and the host's `bolt/x` is still `10e4791`

### Requirement: Every other command reads in full all but the routing
Every command but the protection SHALL read the host's settings in full,
and the policy in full at the revision its gate is read from; to decide
which remote a branch is on, every command SHALL read the policy as the
protection does (124; record 0069).

#### Scenario: Every other command still refuses a field it does not know
- **WHEN** the host's settings hold `[runs] later = 1` and someone runs `sluice status`
- **THEN** it fails naming the settings file and the field, as it did before

#### Scenario: Every other command still refuses a policy field at the branch's revision
- **WHEN** the policy at `bolt/x`'s revision, in local refs, holds a `[[checkout.cache]]` table and a submission to `bolt/x` is waiting
- **THEN** `sluice step bolt/x` lands nothing and names the table, as it did before

### Requirement: Settings or a policy the protection cannot read pass the transaction, said once
Host settings or a policy the protection cannot read at all, whether not
TOML or holding a field guarding needs that is of the wrong kind, SHALL
be a reason of its own: the transaction SHALL pass, with one warning
naming what could not be read and why, and nothing SHALL be refused or
recorded for it (124).

#### Scenario: Host settings that are not TOML pass the transaction
- **WHEN** the host's `~/.config/sluice/config.toml` is not TOML and someone runs `git update-ref refs/heads/bolt/x <unit/a's revision>`
- **THEN** the update succeeds, git's error output holds exactly one warning from sluice saying its branch protection let the transaction through and naming the settings file, and `bolt/x`'s log has no `protected` line

#### Scenario: A remote of the wrong kind passes the transaction
- **WHEN** the policy at `bolt/x`'s revision gives `[branches."bolt/*"]` `remote = 1` and someone runs `git update-ref refs/heads/bolt/x <unit/a's revision>`
- **THEN** the update succeeds with exactly one warning from sluice naming the policy and `remote`, and `bolt/x`'s log has no `protected` line

### Requirement: The hook runs the sluice that installed it
The hook SHALL run the sluice that installed it, by the path it was
installed with, never a `sluice` found where the command that ran git
looks for one, since what the hook refuses may not turn on who ran git.
Every command sluice runs in a repository SHALL re-install the hook
there for itself where sluice's hook stands for another sluice; a
repository with no sluice hook gets one only from `sluice protect`,
`submit`, `wait`, `step` and `run` (1, 124).

#### Scenario: A sluice on the caller's PATH is not the one that judges
- **WHEN** the hook was installed by `sluice protect`, and someone whose `PATH` holds first another program called `sluice` that exits 0 having read nothing runs `git update-ref refs/heads/bolt/x <unit/a's revision>`
- **THEN** git fails with the installed sluice's refusal and `bolt/x` is where it was

#### Scenario: A newer sluice's next command re-installs the hook
- **WHEN** the hook names a sluice at one path, and a sluice at another path runs `sluice status` in the repository
- **THEN** the hook names the second sluice's path, the hook chained beside it is unchanged, and `sluice status` says nothing of the hook

#### Scenario: A removed hook is not put back by a reading command
- **WHEN** `sluice protect --remove` took the hook out and someone runs `sluice status` in the repository
- **THEN** the repository still has no sluice hook

### Requirement: Judging a write fetches nothing
The protection SHALL fetch nothing to judge a write, on any branch. git's
own packing SHALL pass before any of the branch's state is read, and
what still needs a branch SHALL be read from its state as the host last
fetched it (124, S64; record 0110).

#### Scenario: A fetch whose packing runs the protection ends
- **WHEN** `bolt/x` and its state are kept on origin, the repository holds twenty more branches in loose files, and a round on `bolt/x` fetches it, so git's maintenance after the fetch packs the refs and prunes `bolt/x`'s loose file while sluice's fetch holds the repository's fetch lock
- **THEN** the round ends, `bolt/x` is packed at the revision it held, and the protection read none of the branch's state for the packing

### Requirement: Writes of one hooks folder at one time each end well

`sluice host` SHALL write a hooks folder for one request at a time. A
`sluice protect` and a subscription that installs the hook at one time
SHALL each end well. The hook that was there before SHALL stay, chained
beside sluice's (124).

#### Scenario: A protect beside a subscription

- **WHEN** a hooks folder holds a hook of the person's, and eight
  installs of sluice's hook in it start at one time
- **THEN** each install ends well, the hook is sluice's, and the hook
  chained beside it is the person's
  (`installs_at_once_each_end_and_keep_the_hook_there` in
  `crates/sluice/src/protect.rs`)
