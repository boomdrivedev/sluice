---
number: 95
title: A host that does not take a branch's requests in shows each request it follows as the taking host's, from the intake mark and the fields sluice's check carries
status: accepted
date: 2026-11-06
decision-makers:
- the design agent, on 137 and C.5's pull request row
- the design of change/a-host-without-the-intake-mark-shows-where-each-request-stands
---

# A host that does not take a branch's requests in shows each request it follows as the taking host's, from the intake mark and the fields sluice's check carries

Status: accepted. 137 and C.5's pull request row as amended at agentplot/blueprints 4791adf are the design agent's, on crew's fix `fix--a-landing-in-another-hosts-queue-reads-as-in-the-queue`, which looked for such a read and found none at sluice main 0b15d250. D1 to D6 below are the change `a-host-without-the-intake-mark-shows-where-each-request-stands`'s own, under the statement so amended and rules core.25, core.31, core.32 and core.33, which they keep; cheap to change later.

Amended in part by record 0125 (2026-11-13): the host a request is taken in by is the lead the branch's records name, read from them, never from an intake mark, and `taken_in_by` names it alone, with no holder; the mark is kept and read by nothing.

Record 0093, accepted by the user on 2026-11-06 ("I approved 93.") after this change was designed, has no host take a branch's requests in, and the intake mark goes (137 as the statement at 7c2fbf0 proposes it and 88a4cf0 accepts it). What of this record goes with the mark once 0093 is built is under Consequences.

Deciders: the design agent, for the statement; the change's design, for D1 to D6.
Sources: `agentplot/blueprints:design/sluice/requirements.md` clauses 26, 80, 129, 136 and 137 and C.5's pull request row at 4791adf, and 137 at 7c2fbf0 and 88a4cf0; records 0044, 0071, 0088, 0089, 0093 and 0096; rules core.25, core.31, core.32, core.33; crew's `fix--a-landing-in-another-hosts-queue-reads-as-in-the-queue`; the change `a-failed-entry-is-submitted-again` and its record; flume's `crates/flume/src/profiles/merge_queue/sluice/mod.rs`; `crates/sluice/src/profiles/github/pulls.rs` (`queued`, `queue_shown`, `waiting`, `waiting_in_group`, `outcome_shown`, `post_check`, `follow_across`, `Pulls::seen`, `opening`, `hand_over`), `crates/sluice/src/profiles/github/kept.rs` (`Kept::change_readings`), `crates/sluice/src/profiles/pushed_refs.rs` (`intake_mark`, `Mark`), `crates/sluice/src/lib.rs` (`Followed::Open`, the wait's loop) and `crates/sluice/src/overview.rs` as the bolt `requests-show-where-they-stand-from-any-host` held them at 2f0b5c98.

## Context and problem statement

On a branch whose records each host keeps (C.6 local files), the host that holds the branch's intake mark takes its requests in (137). 137 at 4791adf adds:

> The same reads answer a status, an overview or a wait of the branch on a host that does not take its requests in, for every open request that host submitted or reaches (26, 80): each is shown as the taking host's, naming the host the mark names and whether its holder is that host's driver or a wait, and the request's entry there, taken in or waiting and on what, its state, its number on that host and the entries ahead of it, each a field of the JSON form as well as words, and a withdrawal for the hand-over names the taking host as a field (record 0088). sluice's check on the request's head carries those fields for any host to read (C.5), and the mark's host is read from the mark as the tick's fetch brings it (129); neither is read from the other host's records, and a request whose check names no entry yet is shown as waiting to be taken in by the mark's host.

C.5's pull request row at the same commit has the check carry "the entry's state, what it waits for, the host that took it in, its number there and the entries ahead of it, each a field a program reads as well as the check's words (137)".

What sluice did at 2f0b5c98:

- `sluice status <branch>` and `sluice overview` read this host's records alone. A request taken in on another host showed nowhere; one this host handed over showed only as a withdrawn submission, under `--all`.
- A wait whose request another host takes in (`follow_across`) printed the check's title and summary, and under `--json` `{"pull_request", "branch", "state"}`, with `head` and `draft` for `--status`. Nothing named the host, the number there or the entries ahead as fields.
- sluice's check said `queued behind #10, #11` or `testing main + #10 + #12`. It named no host and no number of the request's own, and nothing in it was a field a program reads.
- What a wait last read of a request was kept in its own process (`Pulls::seen`). Two waits on one host each read the request on their own bound, and a status could share neither.

crew needs to tell a landing queued on another host from one queued here, and flume's merge-queue profile, reading `sluice status <base> --all --json`, finds no entry for a request another host takes in and reads it as held. What should the statement ask, how does the check carry the fields, where does a host keep what it follows and how often does it read it, and how do status, the overview and a wait show it?

## Considered options

**What the statement asks (137, C.5).**

1. **sluice's check carries the request's entry as fields, and every host and every tool reads them through sluice's commands** (chosen, the design agent's). One owner: the host that takes a request in posts on its head; every other reader asks sluice.
2. Each tool reads GitHub's check itself. crew and flume would each parse sluice's words, two readings of one thing, against the family's rule of one owner.
3. A host reads the taking host's records. Each host's records are its own and on no remote (C.6); there is nothing to read.
4. status as before, this host's records alone. crew's fix and flume's read find nothing to go on.

**D1. Where the check carries its fields.**

1. **One hidden block at the end of the check's summary** (chosen): `<!-- sluice:queue {"host", "taken_in", "submission", "state", "ahead", "waits_for"} -->`, which GitHub's Markdown hides. `host` is the posting host; `taken_in` whether its records hold a submission of the request at the head the check is on; `submission` and `state` that submission's number and its record's state, `null` while not taken in; `ahead` the submissions ahead of it in that host's queue, each with its request's number or `null` for a worktree's, in the order that host's rounds take the queue (6, 7), priorities and `after`s included, as `sluice status` reads it; `waits_for` what it lacks before that host takes it in, or takes it in again, in the words the title gives. Construction's own: `waits_for` names what a request lacks while it is not taken in, and while its submission at that head left the queue without landing and it waits for its submitter's next submit or its author's ready mark, `taken_in` then true and `state` how it left; emptying it once taken in, the path not taken, would leave that request's way on in its title's words alone. The summary gains a first line, "Submission 1406 on b." or "Waiting to be taken in on b.", and the title stays. The block is part of the posting, so `post_check` posts again only when a field changed (core.32). A reader takes the summary's last block; a check with none carries no fields.
2. The check run's `external_id`, which GitHub defines as the integrator's own reference and documents no length for.
3. A second check run named for the queue: one more post per change and one more read per follow.
4. The check's `text`, which holds the report (record 0044) and is cut to fit; a cut could take the block.
5. A comment on the request: a write per change in the conversation.

**D2. Where the taking host and its queue are read from.**

1. **The host from the intake mark as the host's last fetch brought it, the queue from the check's block on the request's current head** (chosen). `taken_in_by` is `{"host", "holder"}` from `PushedRefs::intake_mark`, with no fetch made for it (129), or `null` where no mark stands; `queue` is the block, or `null`. Neither is read from the other host's records. Where the mark and the check name different hosts, as in the pass between a hand-over and the new holder's first post, both are shown as they read. A request whose check names no submission, because none is on its head, it carries no block, or `taken_in` is false, is shown waiting to be taken in by the mark's host, with what `waits_for` names.
2. The taking host read from the check. The mark decides intake (137), and the check can lag a hand-over by a pass.
3. A fetch at each status, which 129 gives to the tick alone.

**D3. Which requests a host follows, and where it keeps them.**

1. **`forge/follows/<owner>/<repo>/<base>.json`, changed under the `.lock` beside it as the readings are** (chosen), each request kept by number with what was last read of it: when, its head, open, merged and draft, its title, the branch it is opened from, and sluice's check on that head. A request enters when a submit on this host opens, updates or marks it ready where the mark as this host last fetched it names another host, beside the `.moved` touch `opening` leaves; when this host's hand-over withdraws its submission; and when a wait or `submit --status` follows it across hosts. It leaves when a read finds it merged or closed, once a wait or status has said so, or when this host's records take it in at its head. That is 137's "every open request that host submitted or reaches".
2. Asking GitHub for the branch's open requests, which a host that does not take them in never does (137; rule core.33).
3. Reading every worktree on the host for a request opened from its branch: one `find` per worktree, still missing a hand-over.
4. Keeping the list in the branch's records, which hold decisions and would change for reads alone (core.14, core.41).

**D4. How often a followed request is read.**

1. **Once in a quarter of `[forge] discovery` for the whole host** (chosen): `follow_across` reads through the kept file, and where its last read is younger every process uses it; otherwise the process reads the pull and the check on its head, each conditional on the answer kept for its address (136, core.32), and writes what it found. The process that finds the read due claims it under the file's lock, its time set as the read begins, and gives the claim back where the read failed, so two processes never read one request in a quarter. A wait whose pass finds another process of the host holding intake's lock follows the host the mark as last fetched names. `Pulls::seen` keeps only what a wait on the host that takes the request in reads for itself.
2. Each process on its own bound, as before. Two waits read on two clocks, and a one-shot status reads at every call.

**D5. How status, the overview and a wait show them.**

1. **A `PullRequests` method gives a branch's followed requests that this host's queue does not hold at their heads, each read as D4 allows; the trait's default gives none** (chosen), so the core names no forge (core.1, core.2). `sluice status <branch>` prints, after its queue, which host takes the requests in and whether its `sluice run` or a wait holds the mark, or that no host holds it, then a row per request: its number and title (26), and the submission there and what is ahead, or what it waits for. Under `--json` the document gains `taken_in_by` and `pull_requests`, each row `{"pull_request", "title", "from", "head", "draft", "state", "queue", "read_at"}`. A row found merged or closed is shown once and then leaves. The overview gives each such branch the same fields and rows; its one-shot form reads as status does, and a reader's document carries the kept file whenever its modified time moved, its tick reading GitHub for none of it (core.31; rule core.25). A wait's `Followed::Open` carries `taken_in_by` and `queue`; its words gain a line naming the submission there and its host, printed again only when it changes, and its `--status` and timeout documents gain both fields. Construction's own: the line reads `#12 is submission 1406 on b, taken in by b's sluice run`, `#12 waits to be taken in by a wait on b`, or says no host holds the mark; it names no `waits_for`, which the check's title printed beneath it gives, and the check's first line, which says what the line says, leaves the lines beneath the title. A `--no-wait` submit's document and a group submit's undecided items gain both fields as well, and a group's words the same line per request. Construction's own for status: its heading reads `pull requests b takes in, by its sluice run:`, `pull requests b takes in, by a wait on b:` or `pull requests this host follows; no host holds main's intake mark:`; a row reads `#12 <title> · submission 1406 there · queued behind #10`, `on b` in place of `there` where the check's host is not the mark's, `#13 <title> · waiting to be taken in by b: needs 1 review` where the check names no submission, and `landed` or `closed without landing` once, a draft saying so after its title. A request whose latest submission in this host's records, a hand-over's withdrawal aside, is at its head in any state is this host's queue's, as D3 reads a request taken in here, and has no row. The document gains both keys only where it has a row, so a status on the host that takes the requests in, or that follows none, is unchanged, for option 3's reason; a read that fails is warned of, and its row shown as the host last kept it. Construction's own for the overview: it asks for the followed requests of each branch it lists and of each branch the default branch's policy takes pull requests on. A branch this host has no records of is listed in `branches` with its empty queue, as status shows it, after the others, and in the words under `<branch> · no records`. The heading and rows come last under the branch's lines, worded as status words them. A one-shot overview reads them through status's method, and a row it finds merged or closed leaves. A reader's document reads them through a second method of the trait, whose default gives none, which reads the kept file alone, again only once its modified time or the mark's commit moved, the mark's words then read too; it follows none no longer, so a row found merged or closed stays until a status, a wait or a one-shot overview says it. A wait that is the reader writes the same document, whatever its branch. A reader's read that fails leaves its branch without rows and says nothing, since the next one-shot read warns of it.
2. A section of the overview's watch that reads GitHub itself, which core.31 rules out for a reader's tick.
3. Both fields in a wait's documents on every host, `null` on the host that takes the request in: there a `null` `taken_in_by` would read as no mark standing where this host holds it, and the documents a reader of that host already parses would change for nothing.

**D6. Fields or words.**

1. **Where the check carries a block, the request's state is read from it** (chosen): `waits_for` naming the next submit is held, read by the request's draft mark as record 0096 reads a held check, sent back while a draft and waiting to be taken in again once out of it; otherwise `taken_in` with `state` `failed` or `conflict` is sent back, and `landed` is landed. Where it carries none, as an older sluice posted it, the words are read, as `follow_across` and `a-failed-entry-is-submitted-again` read them: its title, never the conclusion GitHub keeps for the run (record 0096). `as_its_check_says` is the one reader of both.
2. The words always. Every reader keeps parsing titles once fields exist.
3. A held block read by its `state` alone, `failed` sent back draft or not. A request its submitter marked ready since would read sent back on another host until the taking host posts again, which record 0096 rules out.

## Decision outcome

The statement's option 1, and option 1 of each of D1 to D6. Rule core.33 holds what they rule out.

### Consequences

- A reader on any host knows from sluice alone which host takes a request in, whether that host's `sluice run` or a wait holds the mark, and where the request stands in that host's queue. A wait, a status and the overview on one host read each followed request from GitHub at most once in a quarter of the interval between them.
- A summary's block is only as fresh as the taking host's last post: a host whose driver stopped leaves its last check standing. `read_at` and the mark's lease (73) tell a reader how far to trust it.
- The kept file grows only with this host's own requests, each leaving once merged, closed or taken in here. Nothing moves for a host without it, and the overview's schema stays `sluice/overview@1`, gaining fields only.
- `a-failed-entry-is-submitted-again` touches `follow_across` too; whichever lands second takes the other's reading, and where both are in, the fields decide.
- flume's merge-queue profile can find its place's request under `pull_requests`, by `from`; that is flume's change, told to the orchestrator. crew's fix reads `taken_in_by` and `queue` from a wait.
- **With the mark, once record 0093 is built.** 0093's 137 keeps this record's reads: a status, an overview or a wait shows each request a host submitted or reaches "with the request's entry there, taken in or waiting and on what, its state, its number and the entries ahead of it, each a field of the JSON form as well as words, read from sluice's check on the request's head (26, 80)". The check's fields (D1), the kept follows from a submit and from a host's own asks with their one bound (D3, D4), `queue` and the `pull_requests` rows (D5) and the fields over the words (D6) stay, the check posted by the instance that keeps the branch on its remote. `taken_in_by` (D2), the words that name the mark's host and its holder, a request waiting to be taken in by the mark's host, and the hand-over as a way a request enters the follows (D3) go with the mark.
