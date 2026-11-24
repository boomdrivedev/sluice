## Context

A branch whose records are on its remote counts a pass only where the
check profile reads, from the run's own records, that the combination's
tree bears its key out (C.6). Before this change, every key the tree did
not bear out failed its check, and the culprit rule blamed the newest
entry of the combination. The decision
`a-key-the-tree-does-not-bear-out-holds-the-branch` takes one cause of
those out of the blame: a key that names a file the tree does not hold,
which is the branch's setup, never an entry's work.

A refused landing already holds a branch: its entries stay theirs, and
the branch waits. This change holds a branch the same way, on a check.

## Decisions

### D1. The check profile names the file

`check_key` fails with `NotHeld { task, path }` where a manifest it
walks names an input the tree lacks, or holds with other content.
`proven` passes `path` on as `Refused.file`. A manifest that does not
hash to its key, names another task, or names no inputs names no file:
that is a key that lies, and it is blamed as before.

### D2. No outcome, and no run again

`finish_run` takes such checks out of the report before anything is
recorded, so nothing is recorded under their keys (rule core.7 records
what the run judged, and the run judged nothing of them). Each is left
with no outcome, its why the profile's words. The candidate's verdict
is `not passed`, blaming no one, and no follow-up run is opened: the
same tree gives the same key, so a run again would fail alike (the
decision's option 3).

### D3. The hold and its entries

`branch.json` keeps one `key_held`: the check, the file, the profile's
words, the branch's revision the refused combination was formed on, the
entries it holds, and since when. A hold that opens is logged once,
`key_held`. Each later combination refused so adds its entries to the
hold and logs nothing.

The entries it holds leave the chain, with each entry whose `after`
names one of them (7). They show as waiting.

### D4. Work not yet tried is tried

The decision has the branch take its entries again at a revision of the
branch that changes the check's inputs. That revision is itself an
entry's work in a queue whose branch moves only by its landings, so an
entry that was in no refused combination is formed and tested as any
entry is. Where its work changes the check's inputs, it passes and
lands, and its landing ends the hold. Where it does not, its combination
is refused alike and it joins the hold, one run spent, nobody blamed.

The entries of the hold are formed again only once it ends; entries
after them land first meanwhile, which a landing beside the chain does
too (140).

### D5. The end

Each round weighs the hold before it forms anything: where no entry it
holds is left in the queue, it ends with no revision; where the
branch's revision is not the one it was read on, the check's affected
reading is asked of the change between them (140). A change that reads
the check, or every check, ends it at the branch's revision; one that
reads it not keeps it. A reading that fails ends it, and its entries
are read afresh. The end is logged once, `key_hold_ended`. Its entries
are formed again on the branch at once, as every candidate is when the
branch moves (10).

### D6. Who is told

`sluice status <branch>` says the hold, with the check, the file, the
entries it holds and the way on; `--json` gives it whole as `key_held`.
The entries' waits go on, so no agent is sent back to work its own
change cannot fix.

## Risks / Trade-offs

- A branch holds until its inputs change; the work waits in the queue
  for that, as the decision accepts.
- An entry that does not change the inputs spends one run before it
  joins the hold.
- A pool's run reads such a key as missing, as before: the hold covers
  a host's runs.
