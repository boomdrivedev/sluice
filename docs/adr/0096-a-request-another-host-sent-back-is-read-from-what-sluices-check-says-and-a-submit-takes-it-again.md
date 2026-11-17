---
number: 96
title: A request another host sent back is read from what sluice's check says, stands nowhere, and a submit takes it again
status: accepted
date: 2026-11-06
decision-makers:
- the construct stage of change/a-failed-entry-is-submitted-again
---

# A request another host sent back is read from what sluice's check says, stands nowhere, and a submit takes it again

Status: accepted, a decision of the construct stage of `change/a-failed-entry-is-submitted-again` under the statement as written (50, 67, 120, 124, 137, C.5's pull request and worktrunk rows) and record 0065, which no clause of it changes; cheap to change later. It reads sluice's check by its words until the check carries 137's fields, which `change/a-host-without-the-intake-mark-shows-where-each-request-stands` builds. It decides for the sluice that keeps the intake mark, until the host process of record 0093 replaces the mark; see "Against record 0093".

Amended in part by record 0125 (2026-11-13): the host that takes a branch's requests in is the one that keeps it on its remote, and a withdrawal for a hand-over is an older sluice's, read as written and never written again.

Deciders: the construct stage of `change/a-failed-entry-is-submitted-again`; the code stage of that change for "Against record 0093".
Sources: `agentplot/blueprints:design/sluice/requirements.md` clauses 50, 67, 120, 124, 137 and C.5's pull request and worktrunk rows at a4eeec3; records 0065, 0071, 0088; rules core.27, core.32, core.33; the signals `signals/2026-11-06-agentplot-planner-944513a6/01-wt-submit-refuses-a-failed-entry` and WilldanGroup/crew-state `wldn/main` `signals/2026-11-06-wldn-ops-ea539623/01-crew-reads-a-pull-request-sluice-sent-back-as-a-stop.md`; WilldanGroup/switchboard-kit pull request #268, its timeline, sluice's check on its head `977138a9` and `refs/sluice/intake/main` as read at 22:52:36Z on 2026-11-06; `crates/sluice/src/lib.rs` (`submit_pull`, `across`), `crates/sluice/src/profiles/github/pulls.rs` (`follow_across`, `opening`, `is_held`, `waiting`), `crates/sluice/src/profiles/github/rest.rs` (`posting_json`, `post_named`), `crates/sluice/src/profiles/github/fake.rs` (`post_named`) and `crates/sluice/src/profiles/worktrunk/mod.rs` (`standing`) as the bolt `sluice-takes-mended-work-again` held them at 0b15d250. For "Against record 0093": record 0093 and rule core.33 as `main` holds them at 36eb1497; clauses 120, 137 and 140 and C.5's pull request row as rewritten on 2026-11-06, at agentplot/blueprints ab5d788 and ratified at 7c2fbf0, as record 0093 cites them, which blueprints' `main` holds as 3f8b23f and a0f0f2f.

## Context and problem statement

At 22:31:11Z on 2026-11-06, `wt submit` refused to submit switchboard-kit's #268 again after sluice sent it back for a failed check: "wt submit: could not read whether a submission stands, so nothing was submitted". Another host takes switchboard-kit's requests in: its intake mark names chuck-sluice-alpha's driver. So `wt submit`'s reading, `sluice submit main --status --json`, followed #268 on GitHub (137). sluice's check on its head was completed and a failure, so that reading took #268 as sent back. It then answered as a waiting submit answers an outcome, with `state` `failed`, no `head`, `draft` or `sends`, and exit 1. `wt submit` reads what stands at `sends` (record 0065, rule core.27) and found none, so it submitted nothing.

The check's words were `waiting: the next submit`, a state sluice posts queued. sluice patches the run it posted on a head, and GitHub keeps a completed run completed, taking only the new words. So once sluice's check on a head has said failed, GitHub says failed on that head whatever sluice posts after: held, queued, under test. A host that reads GitHub's conclusion reads the request sent back until it lands. A submit that marks it ready then reports the old failure at once, and every later wait or status does the same while the other host tests it again. On the host that takes it in, sluice reads its records, and none of this happens.

By 120 a new submit takes a blamed entry again; by record 0065 a failed submission stands at no revision. How does a host that does not take a branch's requests in read a request's state, answer `--status`, and take a sent-back request again?

## Considered options

**How a host that does not take the requests in reads a request's state.**

1. **By the words of sluice's check on its head and the request's draft mark** (chosen). `failed: …` and `conflict: …` are sent back. `waiting: the next submit` is sent back while the request is a draft. Out of draft, it waits to be taken in again by the host the mark names, since only a ready mark ends the draft sluice made (C.5, 120), as `is_held` reads it on that host. Any other words are open. Merged is landed and closed is closed, as before.
2. By GitHub's conclusion, as before. It reads a held request as sent back after a submit marked it ready, and a request taken in again as sent back until it lands.
3. By the request's ready marks against the time the check completed. GitHub's `completed_at` on #268 is the first failure's, so a request blamed twice on one head would read as taken again; and the reading costs a timeline read each pass (rule core.32).
4. sluice posts a new check run whenever its check reopens, so GitHub's conclusion is right again. That changes what GitHub's page and every other reader sees, for every run sluice posts, and wants a design of its own. A held request is queued by design, so it would still be read by its words and draft mark, and runs already completed stay as they are.

**What `--status` answers there.**

- a. **As on the host that takes it in** (chosen): a request landed or sent back is printed with `state` `landed`, `failed` or `conflict`, the check's `title` and `summary`, its `head`, `draft` and `sends`, and exit 0, as `--status` exits wherever it finds the worktree's request.
- b. As a waiting submit answers an outcome, as before: a reading with no revision, and exits a driver would read as outcomes on one host and as answers on the other.

**What `wt submit` reads.**

- i. **A request sent back, failed or in conflict, stands nowhere; one landed at `sends` stands** (chosen), as record 0065 reads a submission.
- ii. Any document without `sends` stands nowhere. An answer sluice did not give would then submit, and withdraw what stands (67).

**What a submit marks there.**

- A. **A request whose check says it was sent back or held is marked ready, draft or not** (chosen), as the host that takes it in marks one whose submission left the queue. That host's records are never read elsewhere (137), so the check says it.
- B. Only a draft, as before. A request opened by hand, or sent back before the other host marks it a draft, stays held, against 120.
- C. Every existing request. A draft mark withdraws a request the other host has queued or under test (C.5).

## Decision outcome

Options 1, a, i and A. The forge double, which replaced a completed run's status on a patch, keeps it completed and takes only the new words, as GitHub did on #268, so the suite meets it.

### Consequences

- `wt submit` run again where another host sent its request back submits it again, and its submit, wait and status agree on the request's state with the host that takes it in.
- The reading rests on sluice's own titles, `failed: `, `conflict: ` and `waiting: the next submit`, held to `pulls.rs`'s postings by a unit test. When sluice's check carries the submission's state as a field (137 as amended at agentplot/blueprints 4791adf), the one function that reads the words reads the field.
- GitHub's page and other readers of the check, crew among them, still see a held or retaken request's check failed on GitHub. That is option 4's, a change of its own.
- A request merged on another host is still found by no `--status`, since `find` reads only this host's history once a request leaves GitHub's open list. A change of its own.
- Rule core.33 says a wait elsewhere follows its request by what sluice's check says, never by the conclusion the forge kept.

## Against record 0093

Record 0093, accepted by the user on 2026-11-06 ("I approved 93."), moves a host's queues into sluice's own host process and rewrites 120, 137, 140 and C.5's pull request row for it. Under it no host takes a branch's requests in. The instance that keeps the branch on its remote does, on a forge its jobs, and the intake mark, its lease and its hand-overs go. Every host follows its request by the forge's state and sluice's check on its head, and the check carries the request's entry as fields (137). Record 0093 supersedes records 0088 and 0089, which this record cites.

This record reads the statement at a4eeec3, which the bolt and every sluice released so far build, and decides for that sluice, the one that keeps the intake mark. `main`'s code keeps the mark and `follow_across` too, so the decision holds there until record 0093 is built. It works around no clause of the rewrite (core.12). Once the host process replaces the mark:

- Option 1's reading by the check's words, never by GitHub's conclusion, is what 137 as rewritten asks of every host. Every host is then one that does not take the requests in, so the across reading becomes every host's.
- Options a and i hold as they stand: `--status` names the request's head, draft mark and `sends`, and `wt submit` reads a request sent back as standing nowhere.
- Option A's mark is C.5's, a submit marking its request ready at the head it sends, from any host.
- `waiting to be taken in by <host>` goes with the mark it names the host from. A held request marked ready then waits for the instance that keeps the branch, and the check's fields name its entry there (137). `as_its_check_says` is the one function that reads the check, so that reading changes there, where `change/a-host-without-the-intake-mark-shows-where-each-request-stands` reads the fields.

This record amends rule core.33 as the bolt holds it, the intake mark's rule. `main`'s core.33, rewritten for record 0093, rules out a host taking requests in and the mark. It does not rule out a wait that follows its request by the forge's state and sluice's check. Where the bolt meets `main`, `main`'s core.33 stands. From this record's amendment, what it rules out carries over, read of every host: a request's state read from the conclusion GitHub keeps, `--status` answering a request decided elsewhere with no `head`, `draft` or `sends`, and a submit leaving unmarked a request its check says was sent back or held. What it says of a host that does not take the requests in goes with the mark.
