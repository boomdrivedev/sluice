## ADDED Requirements

### Requirement: A subscription answers a forecast

A client SHALL ask a forecast on its subscription with a line `{"forecast": "<worktree>", "branch": "<branch>"}`. `sluice host` SHALL answer it on the same connection with one line `{"forecast": {"folder", "branch", "forecast"}}`. The forecast is the document that `sluice submit <branch> --check --json` prints in the worktree. While a forecast is made, `sluice host` SHALL go on sending the documents, the reports and the other forecasts. (`one-subscription-per-plugin@57eb4dfbd366`)

#### Scenario: A slow forecast holds nothing back

- **WHEN** a subscription asks two forecasts, the first slow, and a folder's document changes while the first is made
- **THEN** the client gets the second forecast and the new document before the first forecast

### Requirement: The watch command asks a forecast named on its input

`sluice overview --json --watch --stdin` SHALL take a line `forecast <branch> <worktree>` on its input, and ask that forecast on its subscription. It SHALL print the answer as one line with `folder`, `branch` and `forecast`. (`one-subscription-per-plugin@57eb4dfbd366`)

#### Scenario: A forecast on the watch's input

- **WHEN** `sluice overview --json --watch --stdin` serves a folder and reads `forecast main <worktree>`, for a worktree one commit ahead of `main`
- **THEN** it prints one line whose `forecast` has `"conflict": null` and the worktree's head as `head`, with `folder` the worktree and `branch` `main`
