## MODIFIED Requirements

### Requirement: A submit writes its submission and tells the host process

A submit SHALL send its folder and its worktrees to `sluice host`, which SHALL read each worktree's head, write the submission and answer with its number. A submit with `--no-wait` SHALL then return at once, and a waiting one SHALL follow `sluice host` as `sluice wait` does (135). (`residents-do-the-network-and-git-work@a2d68cf95ffe`)

Before: the submit wrote its submission itself. Now: `sluice host` writes it.

#### Scenario: A submit with no host process running

- **WHEN** no sluice process runs on a host, and an agent runs `sluice submit main --no-wait` in a repository whose `main` holds a policy
- **THEN** the submit exits 0 with the submission's number, and a sluice host process is running
- **AND** that process lands the submission with no other command run

#### Scenario: Ten submits at once

- **WHEN** ten worktrees of one repository each run `sluice submit main --no-wait` at the same moment, with no host process running
- **THEN** each exits 0 with its own number, and exactly one host process runs on the host

#### Scenario: A submit with no git

- **WHEN** `sluice host` runs, and `sluice submit main --no-wait` runs in a worktree with no git on its `PATH`
- **THEN** it exits 0 with the submission's number, and the submission names the worktree's head
