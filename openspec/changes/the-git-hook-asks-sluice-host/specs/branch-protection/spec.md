## MODIFIED Requirements

### Requirement: The hook runs the sluice that installed it

The hook SHALL run the sluice that installed it, by the path that it was installed with. It SHALL NOT run a `sluice` that it finds on the `PATH` of the command that ran git. What the hook refuses SHALL NOT depend on who ran git. A repository with no sluice hook SHALL get one only from `sluice protect`, or where a submit, a wait or a step names it (1, 124).

#### Scenario: A sluice on the caller's PATH is not the one that judges

- **WHEN** the hook was installed by `sluice protect`, and someone whose `PATH` holds first another program called `sluice` that exits 0 having read nothing runs `git update-ref refs/heads/bolt/x <unit/a's revision>`
- **THEN** git fails with the installed sluice's refusal and `bolt/x` is where it was

#### Scenario: A newer sluice's next command re-installs the hook

- **WHEN** the hook names a sluice at one path, and a sluice at another path runs `sluice status` in the repository
- **THEN** the hook names the path of the `sluice host` that answered, which is the second sluice's path
- **AND** the hook chained beside it is unchanged, and `sluice status` says nothing of the hook

#### Scenario: A removed hook is not put back by a reading command

- **WHEN** `sluice protect --remove` took the hook out and someone runs `sluice status` in the repository
- **THEN** the repository still has no sluice hook

## ADDED Requirements

### Requirement: sluice host puts back the hook

`sluice host` SHALL put back the hook in each repository that it binds for a request, where sluice's hook names another sluice. The hook SHALL name the executable of `sluice host`. No command SHALL put back the hook in its own process. (`residents-do-the-network-and-git-work@a2d68cf95ffe`)

Before: each command put back the hook in its own process, by its own path. Now: `sluice host` puts it back by its own path.

#### Scenario: A command with no git

- **WHEN** `sluice host` runs, the hook names another sluice's path, and `sluice status` runs in the repository with no git on its `PATH`
- **THEN** the repository's hook names the path of `sluice host`, and `sluice status` prints what it printed before this change, with no warning of the hook

### Requirement: The hook asks sluice host

The hook SHALL send each transaction in `prepared` to `sluice host`, with its folder, `GIT_DIR` and git's command line, and SHALL exit with the code that `sluice host` answers. It SHALL NOT read a file of the repository, the records or the host's settings. (`clients-never-fetch-or-start-a-tool@8aa9c57f19b2`, `reads-go-through-the-resident@69134e5825d7`)

Before: the hook judged the write from the files. Now: it asks `sluice host`.

#### Scenario: A person's write to a protected branch

- **WHEN** `sluice host` runs, and a person runs `git update-ref refs/heads/main <new> <old>` in the repository
- **THEN** git fails with the refusal's words as the hook gave them before this change, and `main` is where it was

#### Scenario: No sluice host runs

- **WHEN** no sluice process runs on the host, and a person runs `git update-ref refs/heads/main <new> <old>`
- **THEN** a `sluice host` is running, and the write is refused as one with a `sluice host` running

### Requirement: sluice host judges a transaction and writes its refusal

`sluice host` SHALL judge the hook's transaction from the state that it keeps. It SHALL answer without a wait for a round or a reading. It SHALL write each refused write under the branch's lock. (`residents-do-the-network-and-git-work@a2d68cf95ffe`)

Before: the hook wrote the refused write in its own process. Now: `sluice host` writes it.

#### Scenario: The refused write in the log

- **WHEN** a person's `git update-ref refs/heads/main <new> <old>` was refused
- **THEN** `main`'s log has one `protected` line from `sluice host` naming the `git update-ref` command

#### Scenario: A commit on a session's branch during a long round

- **WHEN** `sluice host` holds a round of `main` that runs for 30 seconds, and an agent commits on its own branch
- **THEN** the commit ends within a second of its start

### Requirement: Where sluice host does not answer, the hook refuses

Where `sluice host` does not answer within the hook's bound of ten seconds, the hook SHALL refuse the transaction. It SHALL say that sluice's host process did not answer, and that `sluice protect --remove` takes the protection out.

Before: a transaction that the hook could not read for a reason of its own passed, said once. Now: a transaction that `sluice host` does not judge in time is refused. Settings or a policy that `sluice host` cannot read still pass the transaction, said once.

#### Scenario: A host process that does not answer

- **WHEN** a process holds the socket of `sluice host`, answers `hello`, and does not answer the hook's request, and a person runs `git update-ref refs/heads/feature/y <revision>`
- **THEN** after ten seconds git fails, and its error output says that sluice's host process did not answer and names `sluice protect --remove`

### Requirement: The git commands of sluice host pass by its name

Each git command that `sluice host` runs SHALL carry `SLUICE_HOST_PROCESS`, which names the pid of `sluice host` and its start time. The hook SHALL let that command's transaction go with no ask where that pid runs and started at that time. A check, a command of a session, a preview's command, a release command and an equip step SHALL NOT carry it. (record 0110)

#### Scenario: A fetch of sluice host whose packing runs the hook

- **WHEN** origin keeps `main` and its state, and the repository holds twenty more branches in loose files
- **AND** a round of `sluice host` fetches `main`, and git's maintenance after the fetch packs the refs and prunes `main`'s loose file
- **THEN** the round ends, `main` is packed at the revision it held, and the log of `sluice host` names no request of the hook

#### Scenario: A check that writes main

- **WHEN** `sluice exec main -- git update-ref refs/heads/main <revision>` runs in a worktree
- **THEN** the check's git fails with the refusal, and `main` is where it was

#### Scenario: A name that an ended sluice host gave

- **WHEN** a person runs `git update-ref refs/heads/main <revision>` with `SLUICE_HOST_PROCESS` set to the pid and start time of a `sluice host` that ended
- **THEN** the hook asks `sluice host`, and the write is refused
