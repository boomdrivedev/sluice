## ADDED Requirements

### Requirement: The board holds one subscription for its session

The board SHALL hold at most one `sluice overview --json --watch` at any time, over the session's life. The watch SHALL serve the session's folder alone, with no `--stdin`. A watch SHALL end before the board starts the next one. A later start of the session in one load SHALL NOT start a second watch. (`one-subscription-per-plugin@ba0cdba772ca`)

#### Scenario: A watch that ends and starts again

- **WHEN** the session's watch prints an overview and ends three times, and the board starts the next watch after each end
- **THEN** at no time do two `sluice overview --json --watch` run

#### Scenario: /sluice in a repository with no sluice

- **WHEN** the session's watch printed `"set_up": false` and ended, and the user runs `/sluice` two times
- **THEN** the board starts one watch, and at no time do two run

#### Scenario: The session starts again in one load

- **WHEN** the board's watch is open, and the session's start comes again in the same load
- **THEN** one `sluice overview --json --watch` is running

#### Scenario: The watch's command line

- **WHEN** the session starts in a repository with sluice
- **THEN** the board's watch is `sluice overview --json --watch`, in the session's folder, with no `--stdin`
