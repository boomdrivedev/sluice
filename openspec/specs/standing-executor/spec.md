# standing-executor Specification

## Purpose
Makes a host the operator runs, a box kept warm with its caches, take
runs for the branches that trust its signer: `sluice run` watches the
state on the remote and claims runs of its systems before any pool is
started (88, 107).

## Requirements

### Requirement: A host names itself an executor in its own config
A host's config SHALL say which systems it runs under `[runs]`, as
`systems`, and sign as its `[signing]` signer. A host with no `systems`
SHALL run the one system it is. The repository's policy SHALL never name
a host; only a signer and the claims it may make.

#### Scenario: A box for one system
- **WHEN** a Linux x86-64 host's config has `[signing] signer = "box"` and no `systems`
- **THEN** `sluice run` on it claims only runs needing `linux-x64`, or no system

#### Scenario: A system the host is not
- **WHEN** a Linux host's config has `systems = ["macos-arm64"]`
- **THEN** `sluice run` refuses to start and names `macos-arm64`

### Requirement: The host process claims runs its signer counts for
sluice's host process on a host with `[signing]` SHALL watch the state of
every branch with a remote in the repositories it holds, and claim open
runs of its systems on branches whose gate trusts its signer, up to the
host's bound, while it runs (135). It SHALL claim no run that its
signer's results could not satisfy.

#### Scenario: A session host and main
- **WHEN** a host signs as `sessions` and `main` trusts only `box` and `forge`
- **THEN** the host process on that host claims no run of `main`, and claims runs of a bolt whose `trust` lists `sessions`

#### Scenario: A confined gate and an unconfined signer
- **WHEN** `main` has `confined = true`, trusts `box`, and `box` has `confined = false`
- **THEN** the box's host process claims no run of `main`

### Requirement: An executor wakes on new work
A standing host SHALL notice new open runs within its sign-of-life bound
of their being written, without being told, and sooner when woken.

#### Scenario: A new combination
- **WHEN** a combination is opened on `main` and the box has room
- **THEN** the box claims its run within the sign-of-life bound

### Requirement: An executor's results are its own
A standing host SHALL sign and record its own results on its signer's
ref, and report them under its claim. No other host or job SHALL record
results for it.

#### Scenario: The box records a pass
- **WHEN** the box finishes a run of `main`
- **THEN** its results are on `refs/sluice/runs/box`, signed by the box's key, and the claim reads reported

### Requirement: The overview shows hosts and their room
`sluice overview` SHALL show, for a repository whose state is on its
remote, each host's signer, systems, runs held and room, by its last
sign of life, and mark a host whose sign of life has lapsed.

#### Scenario: A host gone quiet
- **WHEN** host A has not rewritten its sign of life for the bound
- **THEN** `sluice overview` marks host A lapsed and shows none of its room as free
