# Proposal

## Why

On 2026-11-27, the GitHub App used all of its calls for each hour. Two faults of the roadmap come from this:

- "The GitHub App runs out of hourly requests."
- "A `sluice submit --no-wait` exits 3 when the GitHub App's hourly requests are spent."

sluice asks GitHub for the open pull requests of each branch that takes pull requests, one list for each branch (`crates/sluice/src/profiles/github/pulls.rs:778`, `rest.rs:442`). A repository with `main` and bolts that take pull requests asks once for each branch in each interval. The reconcile of the narrative `the-residents` with the code marks this as a break, in part, of `one-forge-read-per-repository-per-interval@b3b6757cea2d`. Its words are "a forge read for each bolt and each pass". The reconcile is `design/research/2026-11-27-the-residents-reconcile.md` in boomdrive.

sluice reads GitHub through ureq and backon (`crates/sluice/src/profiles/github/app.rs:279`). No limit holds the calls in flight at one time across the host. The accepted decision `sluice-host-reads-github-through-octocrab-with-tower-layers@57c2759bda97` says that `sluice host` reads GitHub through octocrab. Its tower layers hold one limit on calls at one time, the budget from GitHub's headers, a retry and a timeout. The reconcile marks it as a break.

The person approved this work on 2026-11-27. The card was "Approve the process model's 24-unit plan, GitHub budget first, and start units 1 and 6–9 now?". On the same day, the person said to build on maintained libraries and not on code written by hand. So this change also does the work of the plan's unit 23.

Claims that this change makes hold:

- `one-forge-read-per-repository-per-interval@b3b6757cea2d`: one list of a repository's open pull requests serves every branch of the repository.
- `open-requests-are-asked-at-the-hosts-interval-or-after-a-move@8b2e884a3f89`: it holds as before, for the repository's list.
- `sluice-host-reads-github-through-octocrab-with-tower-layers@57c2759bda97`: each call to GitHub goes through octocrab, over tower's limit, tower's timeout and a layer that reads the budget.
- `one-github-client-per-host@786dccccffe1`: the limit is one for the process, and `sluice host` is the one GitHub client on a host.

## What Changes

- sluice asks GitHub for every open pull request of a repository in one list, `GET /repos/<owner>/<repo>/pulls?state=open`, with no `base`. Each branch reads its own requests from that list.
- The host keeps when the repository's list was last asked for, in `requests/<owner>/<repo>/.listed.json` under the host's forge state. The interval counts from that time. Any process of the host reads it.
- A pass lists a repository one time at most. A move on the host still asks at once, and that one list serves each branch of the repository in the pass.
- A list that this process holds and that is newer than a branch's last reading serves the branch with no call.
- A branch's own list changed only where its own requests moved. A list that GitHub answered with `304` changed no branch.
- Each call to GitHub's API goes through octocrab. Under it, a tower stack holds:
  - one limit of four calls in flight at one time for the process, on one semaphore.
  - a timeout of `retry::TRY_FOR` for an answer.
  - a layer of sluice's that reads the budget from GitHub's headers onto the answer.
- hyper's client sends the calls over rustls on ring with the webpki roots, and follows no redirect.
- The retry stays sluice's one retry policy around each call (rule core.29).
- ureq stays only for the API of the container engine.

Out of scope:

- The GraphQL query that reads a branch's open requests whole. sluice asks it only when the branch's list changed or a request waits on its conditions (rule core.32).
- `sluice forge check`, which reads GitHub in the command with the operator's own token (rule core.25 lets it). The plan's unit 4 and unit 5 move the tokens to the runner's socket.
- The exit 3 of a submit that does not wait while a pause stands. The accepted claim `a-spent-forge-allowance-pauses-every-process-on-the-host@e7d558ab834a` asks for it. This change removes the cause: idle branches no longer spend the allowance.

## Capabilities

### New Capabilities

- `forge-client`: how sluice reaches GitHub's API: octocrab, one limit on calls in flight for the process, a timeout, the budget layer, and no redirect.

### Modified Capabilities

- `forge-reads`: one list of a repository's open pull requests, at the host's interval or after a move, for every branch of the repository.

## Impact

- `Cargo.toml`, `crates/sluice/Cargo.toml`: octocrab, tower, hyper-util, hyper-rustls, http-body-util and http.
- `crates/sluice/src/profiles/github/client.rs`: the client and its tower stack.
- `crates/sluice/src/profiles/github/app.rs`: each call goes through the client. `resolving` becomes `rerouting`.
- `crates/sluice/src/profiles/github/forge.rs`, `rest.rs`, `fake.rs`: `open_pulls` and `listed` read the repository's one list.
- `crates/sluice/src/profiles/github/pulls.rs`: intake lists the repository once.
- `crates/sluice/src/profiles/github/kept.rs`: the repository's last asking.
- `crates/sluice/tests/profiles/pull_requests.rs`, `forge.rs`, `forge/github.rs`: the tests and the fake's allowance.
