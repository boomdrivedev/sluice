## MODIFIED Requirements

### Requirement: sluice host answers each read from the state that it keeps

`sluice host` SHALL answer a status, an overview without `--watch`, the home view, a forecast, `sluice inbox`, `sluice previews` and a wait's record. Each answer SHALL be the command's output on standard output and standard error, and its exit code. The client SHALL print the answer and SHALL exit with that code. (`reads-go-through-the-resident@69134e5825d7`)

Before: `sluice previews` read the previews' records in its own process. Now: `sluice host` answers it.

#### Scenario: A status in text and in JSON

- **WHEN** `#4` is queued on `main`, and `sluice status main` and `sluice status main --json` run
- **THEN** each prints `main`'s queue with `#4`, as a status printed it before this change, and exits 0

#### Scenario: The home view

- **WHEN** a worktree submitted `#4` to `main`, and `sluice` runs in that worktree with no command
- **THEN** it prints the worktree's home view with `#4`, and exits 0

#### Scenario: The previews

- **WHEN** a preview of `main` is up, and `sluice previews --json` runs
- **THEN** it prints the preview as `sluice previews --json` printed it before this change, and exits 0
- **AND** the command ran no git but the upkeep of the branch protection's hook
