# owed-outcomes Specification

## Purpose
Shows what a branch owes its readers that none has confirmed, owed or
expired, with each hand-over, in the branch's status, and a decided
submission's delivery in every document that gives the submission (51;
weir 104 to 106, 111; S77).

## Requirements

### Requirement: A branch's status shows what is owed

`sluice status <branch>` SHALL show, after its queue, an `OWED` section
listing each reader with something owed or expired on the branch, the
reader marked `never confirmed` where no reader has ever confirmed
under its name, and under it each of its deliveries by key and
submission, `owed` or `expired` with the time it expires or expired,
and each hand-over's way and time. It SHALL show no such section where
nothing is owed (51; weir 104 to 106, 111; S77).

#### Scenario: An expired blame of a removed worktree

- **WHEN** submission 13 from worktree branch `unit-b`, which never
  confirmed an outcome, was blamed on `main` and handed over by
  `notice`, its worktree was removed, and its span has passed
- **THEN** `sluice status main` shows `unit-b` marked `never
  confirmed`, and under it `13:failed` `expired` with its expiry and
  the hand-over by `notice`

#### Scenario: Nothing owed

- **WHEN** every outcome decided on `main` has been confirmed
- **THEN** `sluice status main` shows no `OWED` section

### Requirement: The status document lists what is owed

`sluice status <branch> --json`, with `--all` or without, SHALL hold
`owed`, a list, `[]` where nothing is owed, each element a reader with
`reader`, `known`, and `deliveries`, each with `key`, `id`, `state`
(`owed` or `expired`), `expires_at` and `handed`, each hand-over's `by`
and `at`. These SHALL be fields added to the document as it is.

#### Scenario: One owed landing

- **WHEN** submission 12's landing on `main` is owed to `unit-a`, which
  has confirmed an earlier outcome, and was handed over by `inbox`
- **THEN** `sluice status main --json` holds `owed` with one element:
  `reader` `unit-a`, `known` true, and `deliveries` holding `key`
  `12:landed`, `id` 12, `state` `owed`, its `expires_at`, and `handed`
  with `by` `inbox`

### Requirement: A decided submission's document gives its delivery

`sluice status <n> --json`, `sluice wait --json`, a waiting `sluice
submit --json`, and each element of `sluice inbox --json` SHALL give a
landed, failed or conflicting submission's `delivery`: `key`, `reader`,
`state` (`owed`, `expired` or `done`), `recorded_at`, `expires_at`,
`handed` with each hand-over's `by` and `at`, `confirmed` (a time or
`null`) and `known`; one let go of as `key` and `state` `done` alone.
None of them SHALL hold `delivered` or `delivered_to`.

#### Scenario: A landing pushed and not confirmed

- **WHEN** submission 12 landed on `main` and the round pushed its
  notice into its pane, and `sluice status 12 --json` runs
- **THEN** its `delivery` holds `key` `12:landed`, `reader` `unit-a`,
  `state` `owed`, `handed` with `by` `notice`, and `confirmed` `null`,
  and the document holds no `delivered`

#### Scenario: A landing confirmed and let go of

- **WHEN** `unit-a` confirmed `12:landed`, and 12 has moved to the
  finished state
- **THEN** `sluice status 12 --json` gives `delivery` as `key`
  `12:landed` and `state` `done` alone

### Requirement: A submission with no reader gives no delivery

A submission by name, a withdrawn one and one not decided SHALL give no
`delivery` in any of those documents, but for one whose landing the
forge refused, which gives its refusal's (124, 152; rule core.50;
record 0124 as amended 2026-11-16).

#### Scenario: A queued submission

- **WHEN** submission 15 is queued on `main`, and `sluice status 15
  --json` runs
- **THEN** its document holds no `delivery`

#### Scenario: A landing the forge refused

- **WHEN** the forge refused submission 1's landing on `main`, and
  `sluice status 1 --json` runs
- **THEN** its `delivery` holds `key` `1:refused`
  (`a_landing_the_forge_refused_is_told_with_its_words` in
  `crates/sluice/tests/cli.rs`)
