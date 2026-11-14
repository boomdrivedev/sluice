# Pull requests

**Choose it when** the team lands on the branch through pull requests
today, so it keeps them, or when work should get a pull request on GitHub
before it lands: for review, for other systems' checks (QA, CI, a
deployment), or so each landing has a page. Needs
[remote-branches](remote-branches.md) and the [github-app](github-app.md).

## Keys

```toml
[signers.box]
ssh = ".config/sluice.boxes"   # one line per host's key; see peers.md

[branches.main]
remote = "origin"
required = ["#check-fast", "#check-tools"]   # what sluice tests
trust = ["box"]                              # the signers whose runs count

[branches.main.pull_requests]
reviews = 1                  # approving reviews on the latest commit
checks = ["qa/approved"]     # checks other systems post, by the name GitHub shows
log_lines = 40               # optional: post the end of a failed test's output
```

- `reviews` and `checks` decide when sluice takes a pull request, never what
  it tests: that is `required`.
- They are read from the branch itself, so a pull request cannot loosen its
  own.
- The table needs `remote`; sluice refuses it without one.
- The policy needs `[signers]` ([peers](peers.md)): the branch's state is
  then on its remote, and the host that keeps it there takes its pull
  requests in. Without them each host keeps its own state, and sluice
  refuses that branch alone, a submit to it exiting 2 before anything is
  pushed or opened: `main takes pull requests, which only the host that keeps it on origin takes in, and its state is each host's own: name [signers] in .config/sluice.toml so its state is kept on origin (137)`.
- `log_lines` posts that many last lines of a failed test's output in the
  pull request's report, scrubbed of what looks like a secret. Leave it out
  unless the tests' output is safe for every reader of the repository: the
  report then says where the output is kept on the host.

## How it behaves

- `sluice submit` / `wt submit` push the worktree's branch and open or update
  its pull request instead of queueing the head.
- A pull request waits while it is a draft or misses a review or check, then
  joins the queue. It never waits inside the queue.
- One a submit opened names its agent in its description. Sent back
  (blamed, timed out, in conflict, withdrawn), or moved by commits no submit
  pushed, sluice marks it a draft, and it waits for the next submit from its
  branch or a person's ready mark: `waiting: the next submit`. A submit
  marks it ready for review at the commits it pushes, the same ones
  included.
- One a person opened waits for its conditions again on new commits; sent
  back, or withdrawn by the operator or a planner, it is taken again on new
  commits, or once marked a draft and then ready for review. Withdrawn, its
  check names who withdrew it: `waiting: withdrawn by the operator`.
- Where several hosts reach the branch, the host that keeps it on its
  remote takes its pull requests in. Every other host asks GitHub nothing
  of them and withdraws nothing, and `sluice status` there shows the
  requests it follows, with the host that keeps the branch.
- sluice posts one check named `sluice` with the state (`waiting: …`,
  `queued behind …`, `testing …`, `failed: …`, `conflict: …`, `landed as …`)
  and comments when it sends one back.
- Once a combination has a result, sluice posts its report: one comment in
  the conversation, edited as it changes, the same text as the `sluice`
  check's details, and a check per required test per system
  (`sluice / app:test (linux-x64)`). Don't require these checks in a
  ruleset.
- The app needs Checks and Pull requests read and write; `sluice forge
  check` says so.
- Pull requests from forks are never taken.

## Who notices them

sluice starts its own host process when work arrives, which reads open
pull requests at each pass and moves the queue, and ends once nothing is
in play; nothing is installed for it. A pull request opened by hand has
nobody waiting on it: the host process notices it while it lives, or once
the next command of sluice's starts it. A watch kept going, `sluice
overview --json --watch`, keeps it noticing.

## GitHub settings

- Leave GitHub's merge queue off.
- Required approvals and status checks move from the ruleset, or branch
  protection, into `[branches.main.pull_requests]`: read them from GitHub,
  write them as `reviews` and `checks`, then remove them there on the
  user's word.
- No setting lets GitHub's merge button land while sluice only tests:
  what the button lands was never tested with what landed just before it.
  Asked for one, say there is none, and why, and offer this table.
- Turn on "dismiss stale pull request approvals" if new commits need a new
  review.
- The repository's own CI may keep running on pull requests; name its job in
  `checks` if it must pass first.

Reference: <https://agentplot.github.io/sluice/guides/pull-requests/>.
