## MODIFIED Requirements

### Requirement: A tool that watches the host reads them from sluice

The site's commands page and the setup skill SHALL tell an operator, and
a tool that watches the host, to read refused writes from `sluice
overview --json`'s `branches[].refused_writes`, or `sluice status
<branch> --json`'s `refused_writes`, and never from `log.jsonl` or
`branch.json` (148; the user, 2026-11-05). What the pages say is kept by
their writers and reviewers; no task reads them (principle 16; core.21).

#### Scenario: The pages say where

- **WHEN** an operator reads the overview's fields on the commands page
  and the traps of the remote branches profile
- **THEN** each names `refused_writes` and says not to read the log for
  them
