# Tasks

Each check runs through sluice: `sluice exec main -- <command>`. A test of `crates/sluice/tests/profiles/pull_requests.rs` or `forge.rs` runs in the task `cli:tools`. A test in the crate's source runs in the task `cli:test`.

## 1. One list for each repository

- [x] 1.1 Give `Forge` the method `open_pulls(repo)`. Make `listed(repo)` and `pulls(repo, base)` read the repository's one list, at `/repos/<owner>/<repo>/pulls?state=open` (D1). Make the fake GitHub answer a list with no `base` with every open request. Check: `the_list_says_whether_it_changed` and `every_read_is_conditional` in `forge.rs`.
- [x] 1.2 Keep the repository's last asking in `requests/<owner>/<repo>/.listed.json` (D2). Check: `the_host_sets_the_interval` and `nothing_moved_asks_once_a_minute`.
- [x] 1.3 In intake, ask for the repository's list one time in a pass at most (D2). Serve each branch from a list newer than its reading (D3). Check: `a_repositorys_branches_share_one_list_each_interval`, which fails on main's code with 3 lists where it asks for 1.
- [x] 1.4 Read a branch's list as changed only where its own requests moved (D4). Check: `a_branchs_list_changed_only_where_its_requests_moved`.
- [x] 1.5 Give the fake GitHub an allowance of calls. Check: `idle_branches_leave_the_allowance_for_a_submit`, which fails on main's code: the idle passes spend 6 calls, and the submit exits 3.

## 2. octocrab over tower

- [x] 2.1 Add octocrab, tower, hyper-util, hyper-rustls, http-body-util and http to the workspace and the crate.
- [x] 2.2 Add `client::Client`: octocrab over hyper's client, under tower's global limit of four, tower's timeout and `BudgetLayer`, with no redirect (D5). Check: `six_calls_at_one_time_reach_github_four_at_a_time` and `the_budget_is_read_from_the_headers`.
- [x] 2.3 Send each call of `App` through the client, and keep the retry policy around it (D6). Replace `App::resolving` with `App::rerouting` (D7). Check: `a_comment_whose_first_try_cannot_connect_is_posted_once`, and the redirect's test in `forge.rs`.

## 3. The change

- [ ] 3.1 Run `openspec validate sluice-host-reads-the-forge-once-per-repository-per-interval --strict`, and lint each Markdown file of the change with ste-lint, 0 hard findings. Check: `sluice exec main -- moon run root:specs root:changes`.
- [ ] 3.2 Run the crate's and the root's tasks: `sluice exec main -- moon run cli:test cli:tools root:format root:lint root:specs root:changes`.
