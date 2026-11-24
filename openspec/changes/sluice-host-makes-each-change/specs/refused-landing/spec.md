## REMOVED Requirements

### Requirement: A landing the forge refused is held with its words, never pushed again by itself

**Reason**: By the user's answer of 2026-11-23, `sluice host` pushes a held landing again by itself at each round, and `sluice push` goes.

**Migration**: None. The requirement "A landing the forge refused is held with its words and pushed again at each round" takes its place.

### Requirement: The operator pushes a held landing again

**Reason**: By the user's answer of 2026-11-23, `sluice push` goes. `sluice host` pushes a held landing again at each round.

**Migration**: An operator who ran `sluice push` after a change to the forge's rule runs nothing. The next round pushes the landing.

## ADDED Requirements

### Requirement: A landing the forge refused is held with its words and pushed again at each round

A landing that the forge refused SHALL be written refused in the branch's records with the forge's words. `sluice host` SHALL push it again at each round of the branch, as the same landing, until the forge takes it or one of its submissions leaves. Nothing SHALL be formed again for it, and the submissions behind it SHALL wait. (152, record 0124 as amended 2026-11-23)

Before: only `sluice push` pushed it again. Now: each round pushes it.

#### Scenario: Refused every round

- **WHEN** the forge refuses `#2021`'s landing on `bolt/x` with the same words at each push, and three rounds run after it
- **THEN** each round pushed the landing, `log.jsonl` holds one `forge_refused` line with the forge's words, and `sluice status bolt/x` shows the landing held with them

#### Scenario: The rule changed

- **WHEN** `#2021`'s landing on `bolt/x` is held, and the forge's rule changes so that the forge takes the landing
- **THEN** the next round pushes the landing, and `#2021` lands as the same landing with no run

#### Scenario: The submissions behind it

- **WHEN** `#2022` is queued on `bolt/x` behind `#2021`'s held landing
- **THEN** nothing is formed for `#2022` while the landing is held, and its status says that it waits behind the landing that the forge refused

#### Scenario: sluice push is no command

- **WHEN** a person runs `sluice push bolt/x`
- **THEN** sluice says that `push` is no command, exits 64, as for any command that sluice does not know, and pushes nothing

### Requirement: A held landing's refusal is logged once for each of the forge's words

The refusal of a held landing SHALL be logged once for each distinct words of the forge, and never once for each round. The landing's status SHALL show the forge's latest words. Nobody SHALL be blamed for it, and it SHALL decide no submission's outcome. (152, record 0124 as amended 2026-11-23)

#### Scenario: Refused again with other words

- **WHEN** `#2021`'s landing is held with one set of the forge's words, and the next round's push is refused with other words
- **THEN** `log.jsonl` gains one more `forge_refused` line with the new words, and `sluice status 2021` shows the new words

## MODIFIED Requirements

### Requirement: A held landing's submissions are their submitters'

A submission of a landing that the forge refused SHALL no longer be landing. Its submitter, its planner or the operator SHALL be able to withdraw it. A submission from its place SHALL withdraw it, as it withdraws each earlier submission of that place (5, 67). When one of the landing's submissions leaves, the write that takes it out SHALL drop the landing. The submissions that remain SHALL go back to the queue, and SHALL be formed again on the branch (24).

#### Scenario: The fixed head from the same place

- **WHEN** #2021's landing is held, and its place submits #2022 with the
  test key removed
- **THEN** #2021 is withdrawn, the landing is dropped, and #2022 is
  formed and tested on `bolt/x`
  (`a_submission_from_its_place_clears_a_held_landing` in
  `crates/sluice-conformance/tests/landing.rs`)

#### Scenario: One of two leaves

- **WHEN** a held landing carries #5 and #6, and #5 is withdrawn
- **THEN** the landing is dropped and #6 is queued again and formed on
  the branch (`a_held_landing_is_dropped_once_one_of_its_submissions_leaves`
  in `crates/sluice-conformance/tests/landing.rs`)

#### Scenario: A push after a withdrawal

- **WHEN** a held landing carries #5 and #6, #5 is withdrawn, and the
  next round runs
- **THEN** the round pushes nothing of the held landing, #5 stays
  withdrawn, and #6 is formed on the branch and lands
  (`a_round_after_a_withdrawal_lands_nothing_withdrawn` in
  `crates/sluice-conformance/tests/landing.rs`)
