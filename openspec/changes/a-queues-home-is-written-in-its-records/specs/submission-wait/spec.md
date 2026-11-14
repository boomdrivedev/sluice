## ADDED Requirements

### Requirement: A wait follows its submission to its queue's new home

A wait SHALL follow its submission to where it lives: where the home it
read the submission in is closed by a move, it SHALL read the closed
home's pointer and go on with the submission in the new home, under the
number it has there. It SHALL never work out what carried the
submission, and its document SHALL be the submission's record in the
new home, naming `was` and `carried_from` where its number changed
(record 0122; 26; rule core.48).

#### Scenario: The queue moves while a wait follows

- **WHEN** `sluice wait 52` follows #52 on `bolt/x` in this host's own files, and the branch's queue moves to origin before #52 is tested
- **THEN** the wait goes on following #52 on origin, prints `passed · landed as <rev>` once origin's lead lands it, and exits 0

#### Scenario: Carried under another number

- **WHEN** `sluice wait 52 --json` follows #52 on this host, and the move carries it to origin as #61 because origin's `bolt/x` already held a #52
- **THEN** the wait follows #61, never origin's own #52, and its document is #61's record naming `was` 52 and `carried_from` this host's own files
