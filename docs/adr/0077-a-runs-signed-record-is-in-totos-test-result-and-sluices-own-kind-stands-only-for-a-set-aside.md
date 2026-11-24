---
number: 77
title: A run's signed record is in-toto's Test Result, and sluice's own kind stands only for a set-aside
status: accepted
date: 2026-11-02
decision-makers:
- Chuck Swanberg
---

# A run's signed record is in-toto's Test Result, and sluice's own kind stands only for a set-aside

Status: accepted on the user's approval of 2026-11-02, on their card (req_362d6fec), that clause 141 and C.6's sentence on the catalogue, added in agentplot/blueprints cfe9e04, go into this repository's rules and records, which asked this record to say which catalogued kind carries a run and where sluice's own kind stands. It amends record 0032's predicate; C.6 names the kinds as amended 2026-11-02.

Deciders: the user, on that approval; the code stage of the chore `the-rules-and-records-match-the-statement`, for which kind and where each part of a run goes, read from in-toto's catalogue and agentplot/blueprints 7beb5fa.
Sources: statement clauses 15, 16, 38, 75, 79, 108, 109, 116, 119, 138, 139, 141, C.2 and C.6 (141 added 2026-11-01; C.6 as amended 2026-11-01 and 2026-11-02) in agentplot/blueprints `design/sluice/requirements.md`; agentplot/blueprints 7beb5fa, which gives a test signed for a release in-toto's Test Result; rules core.9, core.22; ADR 0032; in-toto/attestation v1.2.0, `spec/predicates/test-result.md` and `spec/v1/resource_descriptor.md`; `crates/sluice/src/signing/statement.rs` (`RUN_PREDICATE`, `Run`, `Claims`) and `crates/sluice/src/profiles/remote_refs.rs` (`set_aside`) on main at 2026-11-02.

## Context and problem statement

Record 0032 made a run's record an in-toto statement in a DSSE envelope whose predicate type is sluice's own, `https://agentplot.github.io/sluice/run/v1` (`RUN_PREDICATE`). Clause 141 now asks that every record a signer signs be of a kind a public catalogue defines wherever one carries what the record says, sluice defining its own only for a record none carries, and C.6 names the catalogue: in-toto's predicates. A release's gate reads sluice's runs with the verifier it reads its own claims with, and 7beb5fa gives a test in-toto's Test Result there.

What a run's record says: one subject per check and system, `<check>@<system>`, its digest the check's key, which holds the executor's grant (138); each check's outcome, observed or answered from an earlier run (16, 38, 108), with what the tool reported of it beyond the outcome (119); the claims of 79, the executor's kind, whether the run was confined and its system; the branch, the combination, the claim it ran under (75) and the time; and what its executor refused the run, where its record names that (139). Which catalogued kind carries that, and what is left for sluice to define as its own?

## Considered options

1. Keep `run/v1`, sluice's own.
2. in-toto's Test Result (`https://in-toto.io/attestation/test-result/v0.1`), what it names no field for kept in the annotations in-toto's resource descriptors hold.
3. SLSA's Verification Summary Attestation.
4. in-toto's SCAI attribute assertion.

## Decision outcome

Option 2. A Test Result is "one invocation of a test suite", which "may include the results of several individual tests": a run of a combination's checks. Its subjects are "the source artifacts tested", and a key is the identity of one check over the inputs it reads, so each subject names exactly what its check read. The catalogue's framework keeps `annotations` on every resource descriptor, a subject included, for "additional information … useful to the consumer when evaluating the attestation against a policy", which is what the claims are.

**Where each part goes.**

- `_type`, the DSSE envelope, the SSH signature under namespace `sluice-run` and a forge's job signing keylessly stand as record 0032 has them; `predicateType` is the Test Result's.
- The subjects are as now: one per check and system, `<check>@<system>`, digest the key.
- `result` is `FAILED` where any check the run observed failed, `PASSED` otherwise; `WARNED` is never written, since a check passes or fails.
- `passedTests` and `failedTests` list the subjects' names of the checks the run observed. A result the tool answered from an earlier run is in neither, since it is no observation (16, 38); its subject is annotated as a replay with the outcome the tool reported, which counts only through a pass signed for its key (108; core.9).
- `configuration` holds two descriptors: the combination, named `combination`, digest `gitCommit` its revision, annotated with its branch; and the executor, named `executor`, annotated with the claims of 79, the claim the run ran under where it has one (75), when it finished, and what the executor refused the run where its record names that (139).
- Each subject is annotated with what the tool reported of its check beyond the outcome, its time and attempts (119), which no verdict reads.
- `url` is not written.

A verifier reads a check's outcome from the lists and the claims from the executor's descriptor, and counts a claim only where the signer may make it (79), as now. A reader of in-toto's catalogue who knows nothing of sluice reads which checks passed and failed, on which inputs.

**Where sluice's own kind stands.** A set-aside (116): an operator's record that a recorded run did not judge its check, naming it by its key, its system and when it was recorded, with the reason, who set it aside and when (C.6). No catalogued kind says that. Where a signer signs one, its predicate type is `https://agentplot.github.io/sluice/set-aside/v1`. Any record sluice gives a signer to sign later takes a catalogued kind first (141).

**Why not the others.** Option 1 is what 141 rules out once a catalogued kind carries a run. Option 3 records a verifier's decision of a subject against a policy; a run decides nothing, the gate does (15, 27), and sluice signs no reading of a gate. Option 4 carries any attribute as free text, so it would make every record catalogued while telling a reader of the catalogue nothing it can check; 7beb5fa reads the catalogue the same way, keeping a review and a promotion out of it.

### Consequences

- Rule core.40.
- Record 0032's predicate is amended: its signers, claims, signing and verification stand.
- The code moves in a later change: `crates/sluice/src/signing/statement.rs` writes a Test Result in place of `RUN_PREDICATE`'s predicate, and a verifier reads both. A signer's ref is only added to (109), so the statements signed under `run/v1` stay there and are read as before; none is written after the change. The subjects do not change, so a key is looked up as before.
- Today a set-aside on a remote is an unsigned file under `refs/sluice/set-aside` (`remote_refs.rs`, `set_aside`), and no signer signs one. This record names the kind a signed set-aside takes and changes nothing of how set-asides are kept now.
- A verifier that reads in-toto's Test Results reads sluice's runs with no predicate of sluice's to learn.

## Amendment, 2026-11-23: sluice defines no signed kind

Decider: the user, on 2026-11-23, who decided that no result is set aside.

Sources: the statement's clauses 116 and 141 and C.6 (as amended 2026-11-23, boomdrivedev/blueprints 9de7d63); rule core.40; the change `sluice-host-makes-each-change`, design D4.

**Before:** sluice's own kind stood for a set-aside: where a signer signed one, its predicate type was `https://agentplot.github.io/sluice/set-aside/v1`. **Now:** no result is set aside (116), so no record says that a recorded run did not judge its check. Each record a signer signs is of a kind in in-toto's catalogue, a run's a Test Result, and sluice defines none of its own. A later record takes a catalogued kind first, as 141 says.

No signer ever signed a set-aside: an earlier release kept each one unsigned, under `refs/sluice/set-aside` on a remote and in `set-aside.jsonl` on a host. Those files stay where they are, and nothing reads them. The rest of this record, a run's Test Result and where each part of it goes, stands.
