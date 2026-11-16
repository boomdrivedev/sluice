## Why

Clause 152 shows a landing the forge refused "on each entry the landing
carries", and tells its submitter "as an outcome is told (50)". At
`44e33d4` on `bolt/sluice-is-refactored-and-runs-on-pools`, only the
wait's printer, `report`, adds `refused {words, at}` to a refused
submission's record. `decided_document`, which every other document of
a submission is built on, adds neither `refused` nor a `notice`, since
`Submission::decided` gives none for a refusal, which decides no
outcome. And `sluice inbox` never gives a refused submission at all:
`owed_on` takes only landed, failed and conflicting ones, so the
`<id>:refused` the round records stays owed with no inbox to hand it
over (rule core.31).

flume reads a held submission from `sluice status <branch> --all
--json`, its words from the record's `refused.words`, and the inbox's
from its `notice` (flume record 0071 as amended 2026-11-16).
boomdrive-design ruled that sluice puts `refused {words, at}`, read from
the branch's held landing, on every submission record it prints, and
asked for it on this bolt so it ships with record 0124.

## What Changes

- **Every document of a refused submission carries its refusal.**
  `sluice status <branch> [--all] --json`, `sluice status <n> --json`,
  each element of `sluice inbox --json`, `sluice wait --json` and a
  waiting submit give the record with `refused`, the forge's `words` and
  when (`at`), read from the branch's held landing as the wait reads it,
  and `notice`, the refusal's words as the round pushed them. The wait's
  document is built as the status's is, so it gives a decided
  submission's `notice` too.
- **The inbox hands a refusal over as it hands an outcome.** `sluice
  inbox` hands over a refusal owed under `<id>:refused` while its
  submission is refused, its words read from the held landing once the
  round has pushed its notice; every document of the submission gives
  its `delivery` under that key. The refusal still decides no outcome.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `refused-landing`: every document of a held submission carries its
  refusal and its notice; the inbox hands the refusal over.
- `inbox`: an element's `state` may be `refused`, with `refused`.
- `owed-outcomes`: a refused submission's documents give its delivery.
- `submission-wait`: the document lists `refused` among its states and
  is the status's record, its notice among it.

## Out of scope

- **flume's reading of `refused`.** flume's change
  `a-submission-the-forge-refused-waits-on-its-caller` reads it; flume
  pins sluice 0.93.2 until a release carries record 0124.
- **The text forms.** A status row and a wait already say `held: the
  forge refused its landing:` with the words, and the inbox prints the
  refusal's notice and what the wait prints for it.

## Impact

- `crates/sluice-core/src/line.rs`: `Line::refusal_of`, the held
  landing's refusal of one submission.
- `crates/sluice-core/src/owed.rs`: `owed_on` takes a refused
  submission, its words from the outbox item or the held landing;
  `delivery_of` gives a refused submission's delivery.
- `crates/sluice-core/src/records.rs`: `Submission::notification` names
  a refused submission `<id>:refused`.
- `crates/sluice/src/lib.rs`: `decided_document` and `document`, the
  one builder of a submission's document; `status_json`, `inbox`, the
  status command's arm for a number and `report` build on it.
- Tests: `crates/sluice/tests/cli.rs`,
  `crates/sluice-conformance/tests/landing.rs`.
- Records and rules: record 0124's amendment of 2026-11-16, rule
  core.5's details.
- Sources: clauses 50, 51, 135, 152; rules core.5, core.31, core.52;
  record 0124; flume record 0071 as amended 2026-11-16.

## Touches

```
crates/sluice-core/src/line.rs
crates/sluice-core/src/owed.rs
crates/sluice-core/src/records.rs
crates/sluice/src/lib.rs
crates/sluice/tests/cli.rs
crates/sluice-conformance/tests/landing.rs
docs/adr/0124-a-landing-the-forge-refuses-is-held-for-its-submitters-never-pushed-again-by-itself.md
docs/architecture/core.md
openspec/changes/every-submission-record-carries-its-refusal/**
```
