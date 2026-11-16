---
number: 124
title: A landing the forge refuses is held for its submitters, never pushed again by itself
status: accepted
date: 2026-11-13
decision-makers:
- agentplot-design
---

# A landing the forge refuses is held for its submitters, never pushed again by itself

Status: accepted, agentplot-design's. Rules core.5 and core.29 and the statement's 5, 24, 67 and 134 decide it, and it is cheap to change.

Sources: swb-8-conductor's report from chuck-herdr-alpha, relayed by agentplot-operator on 2026-11-13. switchboard-kit's #2021 on `bolt/a-person-makes-and-uses-their-own-api-keys` passed every check, and GitHub's push protection (GH013) refused its landing for a made-up test key. sluice 0.93.2 then pushed it again every round, `sluice withdraw 2021` answered that it was landing, and #2022, the fixed head from the same place, waited behind it. Also agentplot-ops' reading of the code (signal 2026-11-13-agentplot-ops-b29a54cd/01) and sluice v0.93.2:
- `crates/sluice/src/profiles/pushed_refs.rs`: `leased` reads a refusal at the ref only from a `[rejected]` line, so git's `[remote rejected]` is an error.
- `crates/sluice-core/src/step.rs`: `perform_landing` keeps the intent on an error.
- `crates/sluice-core/src/ops.rs`: `withdraw_in` refuses an entry that is landing.

The statement in boomdrivedev/blueprints, `design/sluice/requirements.md`, clauses 5, 24, 67, 122, 134 and 152; record 0122.

## Context and problem statement

A forge can refuse a push for reasons of its own: a rule on the branch, push protection finding what looks like a secret, or a file too large. git reports it as `[remote rejected]`, and the branch does not move. sluice tells only two answers apart, a move made and a branch found elsewhere (24, 45). Any other refusal is an error that leaves the landing for the next round, which pushes it again and is refused again. Nothing can take the entry out: a withdrawal refuses an entry that is landing, and a submission from the same place (67) cannot withdraw it either. So one refused landing stops the branch until someone edits the records by hand.

Forming the chain again does not help: the same entries form the same tree, and the forge refuses it again.

## Considered options

1. **The landing is held with the forge's words, never pushed again by itself, and its entries are left to their submitters** (chosen).
2. **Each entry of the refused landing fails with the forge's words, and the chain forms again without it.** An entry whose content the forge refuses leaves at once. But when the forge refuses every landing, as when sluice's identity loses the right to push, every entry fails in turn after its tests. Submitters are blamed for what is the operator's to fix.
3. **The landing is pushed again on a timer.** A changed rule is picked up, but the forge's answer is pushed against again and again (core.29), and an entry whose content is refused never leaves.
4. **The chain forms again, as when the branch is found elsewhere (24).** The same tree is formed, tested and refused again, a full test run each time.

## Decision outcome

- **Refused by the forge.** A landing whose push the forge refuses is refused by the forge when the branch, read after the refusal, is still at the revision the landing expected. The branch's records say so, with the forge's words whole, before anything else acts on the branch. The refusal is logged once, never once per round, and shown on the branch and on each entry the landing carries. A branch read elsewhere is still 24's refusal, and one read holding the landing is landed (122).
- **Never pushed again by itself.** No round pushes it again, and nothing is formed again for it. The entries behind it wait.
- **Its entries are their submitters'.** An entry of a landing refused by the forge is no longer landing. Its submitter, its planner or the operator may withdraw it (5), and a submission from its place withdraws it (67), so a fixed head clears the branch with no other act. Once any entry leaves, the landing is dropped, and the entries that remain go back to the queue and form again on the branch (24).
- **Told, not decided.** A wait on one of its entries ends saying that the forge refused its landing, in the forge's words, and its submitter is told as an outcome is told (50). The refusal decides no entry's outcome.
- **Pushed again on the operator's word.** Once the forge's rule has changed, the operator asks for the landing to be pushed again, and it lands as the same landing if the forge takes it.

### Consequences

- An entry whose content the forge refuses holds its branch only until its submitter withdraws it or submits again from its place.
- When the forge refuses every landing, the branch stalls where everyone can see it, with the forge's words, and no one is blamed.
- sluice reads one more answer from a push, refused by the forge. It tells it apart from a branch found elsewhere by reading the branch after the refusal.

## Amendment, 2026-11-16: built

The change `a-runs-life-is-written-first-and-actions-runs-its-checks` builds this record. A landing the forge refuses is written in the landing with the forge's words, each of its submissions `refused`, a submission's state added under `sluice/submission@1`, and logged once, `forge_refused`. A wait on a refused submission ends at once with exit 6, saying the forge refused its landing in the forge's words. The refusal is owed to each submission's reader under `<id>:refused`, and decides no outcome. `sluice push <branch>` pushes the held landing again on the operator's word: it lands, or is held again with the forge's new words and exits 6. *Not taken:* exit 1 for a refused landing, which a reader takes as its submission failed.
