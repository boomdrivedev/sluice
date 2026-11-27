# Design

## Context

Intake asks GitHub for the open pull requests of one branch at a time (`crates/sluice/src/profiles/github/pulls.rs:778`). `forge.listed(repo, branch)` sends `GET /repos/<owner>/<repo>/pulls?state=open&base=<branch>` (`rest.rs:442`). Each branch keeps its own last asking in `requests/<owner>/<repo>/<branch>.json`. So a repository with `main` and two bolts that take pull requests asks three times in each interval. It asks three times again after a move on the host.

The app sends each call through one `ureq::Agent` (`app.rs:279`). The agent has a global timeout and follows no redirect. Nothing holds the number of calls in flight at one time. `sluice host` runs the bindings of many repositories on many threads, so each binding can send its calls at the same time.

The decision `sluice-host-reads-github-through-octocrab-with-tower-layers@57c2759bda97` names octocrab with four tower layers. They are one global limit, a layer of ours that reads the budget from the headers, a retry with backoff, and a timeout.

## Goals / Non-Goals

**Goals:**

- One list of a repository's open pull requests in each interval, for every branch of the repository. (`one-forge-read-per-repository-per-interval@b3b6757cea2d`)
- Each call to GitHub through octocrab, under one limit on calls in flight for the process, a timeout and the budget layer. (`sluice-host-reads-github-through-octocrab-with-tower-layers@57c2759bda97`)

**Non-Goals:**

- One GraphQL query for the whole repository. It stays one for each branch, and only when the branch's list changed or a request waits (rule core.32).
- A change of the pause. The app keeps the pause in the host's state as before (`a-spent-forge-allowance-pauses-every-process-on-the-host@e7d558ab834a`).
- `sluice forge check` in `sluice host`. It reads GitHub with the operator's own token, which rule core.25 lets a command do.

## Decisions

### D1. One list for each repository

`Forge::open_pulls(repo)` gives every open pull request of the repository. `Forge::listed(repo)` gives the same list and whether GitHub sent it afresh. `Forge::pulls(repo, base)` reads the requests into `base` from that list. The REST address is `/repos/<owner>/<repo>/pulls?state=open`, so every reader of the list shares one kept answer and its ETag (`every-forge-read-is-conditional@26479095afff`).

### D2. The repository's last asking

The host keeps the time of the repository's last asking in `requests/<owner>/<repo>/.listed.json`. A branch name never starts with a dot, so the file never meets a branch's readings. Intake asks for the list where one of these holds:

- The interval passed since the repository's last asking.
- A submit on the host moved a request into the branch since the branch's last reading.
- The branch or its records moved since the branch's last reading.
- The branch has no reading, and this process holds no list.

A pass asks for a repository's list one time at most. `Pulls::next_pass` starts a new pass. A second intake of one branch also starts one, because `sluice step` takes one branch in with no pass around it.

### D3. A list held serves each branch

`Pulls` holds the last list of each repository that it asked for, with the time of the asking. A branch whose last reading is older than that list reads its requests from it, and asks nothing. The branch's reading then takes the list's time as its asking, so a later move on the host is newer than it. A process that starts again holds no list. Its branches wait for the next interval, or a move, as before.

### D4. A branch's list changed only where its requests moved

The one query for a branch's requests runs where the branch's list changed (rule core.32). A repository's list changes when any branch's requests move. So a branch reads its list as changed only where GitHub sent the list afresh and the branch's own requests differ from its reading. sluice compares their numbers, heads, last changes and draft marks.

### D5. octocrab over tower

`client::Client` holds a current-thread runtime of its own and an `Octocrab` built with `OctocrabBuilder::new_empty().with_service(..)`. The service is hyper's client, under these layers from the outside in:

1. tower's `GlobalConcurrencyLimitLayer` on one `Semaphore` of the process, `client::GITHUB`, with four places. GitHub takes 100 calls at most from one client, and asks a client to send its calls one after another where it can. Four keeps one repository's slow call from holding every other repository's.
2. tower's `TimeoutLayer` of `retry::TRY_FOR`.
3. `BudgetLayer`, sluice's own: it reads `x-ratelimit-remaining`, `x-ratelimit-reset`, `x-ratelimit-resource` and `retry-after` into a `Budget` on the answer.

The connector is hyper-util's `HttpConnector`, with a connect timeout of `retry::TRY_FOR`, under hyper-rustls on ring with the webpki roots, as ureq had them. No layer follows a redirect, so a renamed repository's `301` stays the answer (rule core.29). The app makes its client at its first call, and sends each call with the whole address.

The app judges a call that got no answer as before. Where hyper-util made no connection, or the system refused one, the call was never sent. Otherwise sluice does not know whether GitHub took it.

### D6. The retry stays sluice's one policy

The decision names a retry layer. Rule core.29 asks for one retry policy for every call to a remote, git's among them. That policy reads answers that a tower layer does not. A `401` buys a fresh token for the next try. A write that makes something is never sent again blind. So the retry stays `retry::Policy` around each call, with backon's backoff, a maintained crate. tower's retry layer is not added. The decision's retry is this policy.

### D7. A test's connection goes elsewhere

`App::rerouting` gives the connector a function that changes each call's address. The test "A connection never made" sends its first try to port 0. This replaces `App::resolving`, which gave ureq a resolver.

## Risks / Trade-offs

- [A runtime for each app] → An app is one binding's, for its life. The app makes its runtime at its first call, so a binding that calls nothing makes none.
- [One semaphore for many runtimes] → tokio's `Semaphore` wakes a waiter on any runtime. Each runtime runs only while its call waits.
- [A restart loses the held list] → A branch with a reading older than the last asking waits for the interval or a move. That is a minute at most.
