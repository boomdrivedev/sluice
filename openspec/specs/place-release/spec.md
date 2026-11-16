# place-release Specification

## Purpose
Says what happens when sluice releases a checkout it made or a
worktree's place it adopted: a release that fails holds the checkout,
never the branch or its host's claims, and a checkout whose working
copy is gone has its stored release run once and is freed whatever it
did (123, 81, 25, 34).

## Requirements

### Requirement: A failed release holds its checkout, named with what failed

When the release of a combination's checkout fails, sluice SHALL keep
the checkout and SHALL record in the branch's state its name, what
failed and how many times its release was tried; the branch's log SHALL
hold a `release_failed` event naming it and what failed (123).

#### Scenario: The release command fails

- **WHEN** a combination's result is read and its checkout's release
  command exits 2 printing `usage: scripts/preview up`
- **THEN** the checkout still exists, `sluice status <branch> --json`
  lists it with that output and one try, and the log holds
  `release_failed` naming it

### Requirement: A failed release is tried again a bounded number of times

sluice SHALL try a held combination's checkout's release again on each
round, as many more times as the branch's `release_retries` states, read
from the branch's current revision, or 3 times when it states none. Once
they are spent sluice SHALL stop trying and SHALL report the checkout
held, once, in the round's output and in its branch's log (123, 56).

#### Scenario: No retries named

- **WHEN** a checkout's release fails on every try and the policy names
  no `release_retries`
- **THEN** the release is run four times in all, over four rounds, and
  the fifth and later rounds run nothing for it and print nothing of it

#### Scenario: Retries named

- **WHEN** the branch's policy says `release_retries = 1` and the
  release fails twice
- **THEN** the checkout is reported held after the second failure, with
  what failed

#### Scenario: A retry succeeds

- **WHEN** a checkout's release fails once and succeeds on the next
  round
- **THEN** the checkout is gone, its ports are free, the branch's state
  no longer lists it, and the log holds `released` naming it

### Requirement: A held checkout never holds its branch

Whatever a release does, the round SHALL go on with the branch's work:
it SHALL land what passed, tell outcomes, and form and start
combinations in other checkouts. A failed release SHALL never be
reported as the submission's outcome (123, 122).

#### Scenario: The queue goes on

- **WHEN** a combination's checkout's release fails and another
  submission is waiting
- **THEN** the same round starts the waiting submission's combination in
  a new checkout, and it lands when it passes

#### Scenario: A checkout left from a run that never started

- **WHEN** a round stopped after deciding a checkout and before its run
  started, and the checkout's release fails on the next round
- **THEN** the run is opened again and started in another checkout, and
  the failed checkout is held as above

### Requirement: A held checkout bars no claim

A host that releases the checkouts it left before it claims (81) SHALL
treat a failed release as above: held, named, tried again, then
reported. Once each checkout it left is released or held, the host
SHALL claim the branch's open runs as usual, up to its bound; a held
checkout SHALL never keep its host from claiming (81, 123).

#### Scenario: Back with a checkout that will not release

- **WHEN** host A returns with a checkout left from a lapsed claim whose
  release fails, and the branch has open runs
- **THEN** host A shows the checkout held with what failed, claims open
  runs in other checkouts in the same round, and reports their results

### Requirement: The operator releases a held checkout by its name

`sluice release <name>` SHALL release the held checkout of that name on
this host, running its release once more; on success the checkout is
gone and its branch's state no longer lists it, and on failure it stays
held with what failed and the command exits 1. A name that names no
checkout SHALL exit 0 saying there is nothing to release (123).

#### Scenario: Released once more

- **WHEN** the operator fixes what the release needed and runs `sluice
  release sluice/try/main/48`
- **THEN** the release command runs, the checkout is removed, its ports
  are free, and the log holds `released` naming it

#### Scenario: Still failing

- **WHEN** the operator runs `sluice release sluice/try/main/48` and the
  release fails again
- **THEN** the command prints what failed, exits 1, and the checkout is
  still listed held

### Requirement: The operator may accept a release's failure

`sluice release <name> --accept-failure` SHALL free what sluice itself
holds for the checkout without running its release command: end its
runs, remove the checkout sluice made, its scratch directory and record,
and let its ports go. The log SHALL hold `released` naming it and the
failure accepted. In a worktree, `sluice release --accept-failure` SHALL
do the same for the worktree's place and leave the worktree (123).

#### Scenario: Accepted

- **WHEN** a checkout is held because its release command fails, and the
  operator runs `sluice release sluice/try/main/48 --accept-failure`
- **THEN** the release command is not run, the checkout is removed, its
  ports are free for the next checkout, and `sluice status main` no
  longer lists it

#### Scenario: Accepted where git no longer lists the checkout

- **WHEN** `sluice/try/main/63` is held because git no longer lists its
  directory as a worktree, the directory still holds the checkout's
  files and a directory a check left read-only, and the operator runs
  `sluice release sluice/try/main/63 --accept-failure`
- **THEN** nothing is run, the directory is removed, its ports are free,
  and `sluice status main` no longer lists it
  (`a_checkout_git_no_longer_lists_is_freed` in
  `crates/sluice/tests/profiles/places.rs` for the checkout, and
  `release_accepting_the_failure_frees_a_held_checkout` in
  `crates/sluice/tests/cli.rs` for the command)

### Requirement: A worktree's or a preview's failed release holds it

A failed `sluice release` in a worktree, or a failed release of a
preview, SHALL leave the place held with what failed: `sluice status`
lists the worktree with it, and `sluice previews` the preview. sluice
SHALL try a preview's release again as a combination's checkout's is,
and SHALL never release a worktree whose directory exists on its own
(123, 25, 101).

#### Scenario: A worktree's release fails

- **WHEN** an agent runs `sluice release` in its worktree and the
  release command exits 1
- **THEN** the command exits 1 with the command's output, the worktree
  keeps its ports, and `sluice status` lists it with what failed, and no
  round releases it

### Requirement: A checkout whose working copy is gone is released once and freed

When the working copy of a checkout sluice holds is gone, whether a
worktree's, a combination's or a preview's, the next run or round on the
host SHALL run its release once, from what sluice itself holds for it:
the release command its last run stored, with its stored environment
and ports, in the checkout's scratch directory, entering no toolchain
and reading no policy. A checkout whose runs stored no release command,
or whose stored release has run already, SHALL have nothing run (123).

#### Scenario: A worktree removed by hand

- **WHEN** an agent's worktree that ran `sluice exec` under a policy
  with a `[tests] toolchain` and a release command is removed with
  `rm -rf`, and another agent runs `sluice submit` and waits
- **THEN** the first round runs the stored release command once, from
  the worktree's scratch directory with the worktree's port in its
  environment, the toolchain is not run, the round says once that the
  worktree is gone and its ports are free, the log holds `gone` naming
  it, and no later round runs its release or mentions it

### Requirement: A gone checkout is freed whatever its release did

Once a gone checkout's release has run, or found nothing to run, sluice
SHALL end its runs and let go its scratch directory, ports and record,
whatever the release did. Its branch's log SHALL hold `gone` naming it
and saying how the release went, and sluice SHALL never run its release
again (123).

#### Scenario: A gone checkout's release fails

- **WHEN** a removed worktree's stored release command exits 2
- **THEN** its ports and record are freed all the same, the log holds
  `gone` with what the command printed, and the release is never run
  again

#### Scenario: A combination's checkout deleted

- **WHEN** a combination's checkout directory is deleted before its
  result is read
- **THEN** on release its stored release command runs once in its
  scratch directory, git no longer lists it as a worktree, its ports are
  free, and the log holds `gone` naming it

#### Scenario: Nothing stored to run

- **WHEN** a removed worktree's runs stored no release command
- **THEN** nothing is run, its ports and record are freed, and the log
  holds `gone` saying no release ran

#### Scenario: A checkout held because git refused it

- **WHEN** `sluice/try/main/63`'s first release ran its release command
  and then failed in git, `Directory not empty`, and its later tries
  were refused with `is not a working tree` until it was held
- **THEN** the next round reads it as gone, runs nothing, since its
  release command ran already, removes what is left of its directory,
  logs `gone` naming it, and `sluice status main` no longer lists it
  (`a_checkout_git_no_longer_lists_is_freed` in
  `crates/sluice/tests/profiles/places.rs` for the checkout, and
  `a_held_checkout_deleted_since_is_released_once_and_freed` in
  `crates/sluice-conformance/tests/places.rs` for the round)

### Requirement: The overview's document carries a branch's held checkouts

`sluice overview --json` SHALL give each branch `held_checkouts`, every
checkout its state names whose release failed, each with `place`,
`failed`, `tries`, `at`, `stopped` and, on a branch whose state is on its
remote, `host`, as `sluice status <branch> --json` lists them. It SHALL
read them from the branch's state alone, so each host's overview shows
them, and SHALL leave the list empty when there are none (123, 26, 61).

#### Scenario: A held checkout in the document

- **WHEN** `main`'s checkout `sluice/try/main/48` has had its release
  fail four times with `usage: scripts/preview up` and is tried no more
- **THEN** `sluice overview --json` lists under `main` a `held_checkouts`
  entry with `place` `sluice/try/main/48`, `tries` 4, `stopped` true and
  `failed` ending `usage: scripts/preview up`

#### Scenario: Another host's overview

- **WHEN** `main`'s state is on its remote, host `deneb` holds
  `sluice/try/main/48`, and the overview is read on host `vega`
- **THEN** `vega`'s document lists the same entry with `host` `deneb`

#### Scenario: Nothing held

- **WHEN** no release of `main` has failed
- **THEN** `main`'s `held_checkouts` is an empty list

### Requirement: The overview names each held place under its branch

`sluice overview` SHALL show under each branch every checkout, worktree
and preview of that branch held because its release failed and tried no
more: its name, how many times its release failed, the last line of what
failed, and the command that runs its release again and the one that
frees it without. A held checkout of another host SHALL name that host.
A branch with no records SHALL get its own line for them (123, 26).

#### Scenario: A held checkout

- **WHEN** `sluice/try/main/48` is held after four failed releases ending
  `usage: scripts/preview up`
- **THEN** under `main`'s line `sluice overview` prints
  `sluice/try/main/48 is held: its release failed 4 times: usage:
  scripts/preview up`, then `sluice release sluice/try/main/48 runs it
  again; sluice release sluice/try/main/48 --accept-failure frees it
  without`

#### Scenario: A held worktree

- **WHEN** the worktree `/work/app/unit-a`, whose runs are held to
  `main`, had its `sluice release` fail once
- **THEN** under `main`'s line the overview names `/work/app/unit-a` as
  held with that failure, and gives `sluice release --worktree
  /work/app/unit-a` and the same with `--accept-failure`

#### Scenario: A held preview

- **WHEN** a preview of `main` had its release fail until its tries were
  spent
- **THEN** under `main`'s line the overview names the preview as held
  with what failed, and gives `sluice release <preview>` and the same
  with `--accept-failure`

#### Scenario: Another host's checkout

- **WHEN** the overview is read on `vega` and `sluice/try/main/48` is
  held by `deneb`
- **THEN** its line says it is held on `deneb`, and its commands are
  given as to run on `deneb`

#### Scenario: Still being tried

- **WHEN** `sluice/try/main/48`'s release has failed once and the next
  round will try it again
- **THEN** `sluice overview` prints nothing of it, and `sluice status
  main` lists it as tried again by the next round

#### Scenario: A worktree's branch with no records

- **WHEN** a worktree held to `release`, a branch with no records, is
  held because its release failed
- **THEN** the overview prints `release · no records` followed by the
  worktree's held lines

### Requirement: Showing a held place changes nothing

Reading the overview, and any view drawn from it, SHALL run no release,
count no try and write nothing; a held place SHALL stay held until the
operator's `sluice release` asks for it (123, 61).

#### Scenario: Read many times

- **WHEN** `sluice overview --json --watch` runs for ten minutes while
  `sluice/try/main/48` is held
- **THEN** its release is never run, its tries stay as they were, and
  the branch's state and log are unchanged

### Requirement: A worktree sluice holds nothing for is released by the policy's release command

`sluice release` in a worktree of the repository that sluice holds no
ports, runs or record for SHALL run the release command named by the
policy of the repository's default branch, read at its current
revision. It SHALL run it at the worktree's top as a held worktree's
release runs it: under the executor the policy's limits choose,
entering the toolchain alone, with a scratch its own record names and no
ports (95, 34, 56; rules core.10, core.30, core.46).

#### Scenario: A release that leaves a mark

- **WHEN** a worktree on `unit/a`, in which `sluice exec` never ran, is
  released with `sluice release`, and `main`'s policy names a release
  command that writes the folder it runs in to a mark file
- **THEN** the mark names the worktree, the command says the worktree
  was released and exits 0, and `sluice status` lists no worktree held
  (`an_unheld_worktrees_release_runs_its_release_command` in
  `crates/sluice/tests/exec.rs`)

### Requirement: An unheld worktree's release answers as a held one's and keeps nothing

The release of a worktree sluice holds nothing for SHALL answer as a
held worktree's release does: `released` and exit 0 when the command
passed, and what the command printed and exit 1 when it failed. Either
way sluice SHALL keep nothing for the worktree once it answers, and a
release asked again SHALL run the command again (95, 123).

#### Scenario: A failing release

- **WHEN** `main`'s policy names a release command that prints
  `emulator still answers` and exits 3, and `sluice release` runs in a
  worktree sluice holds nothing for
- **THEN** the command exits 1 with `emulator still answers` in what it
  says, and `sluice status` lists no worktree held
  (`an_unheld_worktrees_release_runs_its_release_command`)

#### Scenario: Released again

- **WHEN** `sluice release` runs in a worktree whose place it has just
  released, and the policy still names a release command
- **THEN** the release command runs again, and the command says the
  worktree was released and exits 0
  (`release_runs_the_policys_release_command_once` in
  `crates/sluice/tests/cli.rs`)

### Requirement: Nothing to release only where no release command is named

In a worktree sluice holds nothing for, `sluice release` SHALL say
`nothing to release` and exit 0 only where the default branch's policy
names no release command, or where the folder is in no worktree of the
repository. Where the default branch or its policy cannot be read, the
release SHALL fail, exit 1, naming the branch and the words it was
refused with, and run nothing (rule core.10).

#### Scenario: A policy that cannot be read

- **WHEN** `main`'s policy names a key sluice does not know, and
  `sluice release` runs in a worktree sluice holds nothing for
- **THEN** the command exits 1 naming `main`, and nothing is run
  (`an_unheld_worktrees_release_runs_its_release_command`)

#### Scenario: A policy that names no release command

- **WHEN** `sluice release` runs in a worktree sluice holds nothing
  for, under a policy that names no release command
- **THEN** it says `nothing to release` and exits 0
  (`equip_adopts_nothing` in `crates/sluice/tests/equip_tools.rs`)

### Requirement: A checkout or a preview git no longer lists is gone

A combination's checkout or a preview, each made by sluice, SHALL be
read as gone when its directory is gone, and also when git no longer
lists its directory as a worktree of the repository, whether or not the
directory is still there. sluice SHALL match the directory by the
directory itself, never by how its path is spelled, and SHALL start no
process to read it. A reading that fails SHALL never be read as gone
(123; rule core.31).

#### Scenario: A checkout git stopped removing part way

- **WHEN** git's removal of `sluice/try/main/63` stopped part way, so
  its directory still holds the checkout's files while git no longer
  lists it as a worktree
- **THEN** the checkout is read as gone (`a_checkout_git_no_longer_lists_is_gone`
  in `crates/sluice/tests/profiles/places.rs`)

#### Scenario: A checkout git lists under another spelling of its path

- **WHEN** a checkout was made through a path that reaches the
  repository by a symbolic link, and git lists its directory by another
  spelling
- **THEN** the checkout is not read as gone
  (`a_checkout_git_no_longer_lists_is_gone`)

#### Scenario: A preview git no longer lists

- **WHEN** git no longer lists a preview's checkout as a worktree, and
  its directory is still there
- **THEN** `sluice previews` reads it as gone
  (`a_checkout_git_no_longer_lists_is_gone`), and the next pass frees it
  as any gone preview (`a_preview_whose_checkout_is_gone_is_freed` in
  `crates/sluice-conformance/tests/previews.rs`)

### Requirement: A worktree's place is gone only once its directory is

A worktree's place SHALL be read as gone only once the worktree's
directory is gone, whether or not git lists the worktree, so no round or
run releases the place of a worktree whose directory is still there
(123; rule core.16).

#### Scenario: A worktree git no longer lists

- **WHEN** git no longer lists an agent's worktree, which sluice holds a
  place for, and its directory is still there
- **THEN** its place is not read as gone, and no round or run releases
  it (`a_checkout_git_no_longer_lists_is_gone` in
  `crates/sluice/tests/profiles/places.rs`)

### Requirement: sluice removes what is left of a checkout git no longer lists

When sluice frees a checkout or a preview it made, by a round or by
`--accept-failure`, it SHALL remove the directory through git where git
lists it as a worktree, and otherwise SHALL remove what is left of the
directory itself, first making writable every directory in it that its
owner cannot write. A directory it still cannot remove SHALL be a
failure to free, named with the directory and what refused it, and the
checkout SHALL stay held with it (123).

#### Scenario: What git left of a checkout

- **WHEN** git no longer lists `sluice/try/main/63`, its directory still
  holds the checkout's files and a directory a check left read-only, and
  its run stored a release command
- **THEN** the release command runs once from its scratch directory, the
  directory is removed, git lists nothing there, its scratch directory
  and record are gone, and its ports go to the next checkout
  (`a_checkout_git_no_longer_lists_is_freed` in
  `crates/sluice/tests/profiles/places.rs`)
