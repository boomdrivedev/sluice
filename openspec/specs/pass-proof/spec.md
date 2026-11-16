# pass-proof Specification

## Purpose
What a pass needs before it counts on a branch whose state is on its
remote: the task runner's own record that the test ran, kept where the
check profile reads it (C.6). A test that passed in a run that kept no
such record has no result. Nothing is recorded under its key, nobody is
blamed, and its combination is tested again within 115's bound. Why is
read from the missing record itself. The branch's log names each such
cause once per host and reason, with its end, and `branch.json` keeps it
while it stands (114, 115; record 0105).

## Requirements

### Requirement: A pass the run kept no proof of has no result

On a branch whose state is on its remote, where a test ran and passed
and the run kept nothing that proves it ran, sluice SHALL record no
result for it under its key, list it as not run with why, and blame no
submission (114, 16; record 0105; rule core.35).

#### Scenario: Every pass of a run has no proof

- **WHEN** `main`'s state is on `origin`, a run of #1's combination
  passes `a:test` and `b:test`, and the run kept no proof of either
- **THEN** `results.jsonl` gains no line for either test, the branch's
  log records the run with no result and each test not run with why,
  and #1 is not blamed
  (`a_pass_nothing_proves_has_no_outcome_and_runs_again` in
  `crates/sluice-conformance/tests/unkept_proof.rs`)

#### Scenario: A test that failed on its own beside it

- **WHEN** a run of the combination fails `a:test` and passes `b:test`,
  and the run kept no proof of `b:test`
- **THEN** the combination fails on `a:test` as before, and `b:test` is
  named as could not run, with why
  (`a_check_that_failed_beside_a_pass_nothing_proves_fails_as_before`
  in `crates/sluice-conformance/tests/unkept_proof.rs`)

### Requirement: A combination whose passes had no proof is tested again

A combination whose run kept no proof of its passes SHALL be tested
again, and the run SHALL count toward the runs in a row whose tests
could not run, so that the third such run reads the combination as not
passed and blames with why (114, 115).

#### Scenario: The next run keeps its proof

- **WHEN** the combination's next run passes both tests and keeps their
  proof
- **THEN** both passes are recorded under their keys and #1 lands
  (`a_pass_nothing_proves_has_no_outcome_and_runs_again`)

#### Scenario: Three runs in a row

- **WHEN** three runs in a row of the same combination each keep no
  proof of its passes
- **THEN** the third leaves the combination not passed, its newest
  submission blamed, and its session told each test could not run, with
  why (`three_runs_whose_passes_nothing_proves_blame_with_why` in
  `crates/sluice-conformance/tests/unkept_proof.rs`)

### Requirement: Why is read from the missing proof itself

The reason SHALL be read from the fact itself, the task runner's record
missing where the check profile reads it, and SHALL name where it was
looked for and whose record is missing, never what the task runner
printed (record 0105; rule core.35). For moon, the record is moon's
manifest of the test's key and of each key it depends on (C.2 moon
row), and the reason SHALL read `it passed, but nothing the run kept
proves it ran: moon's manifest of <whose> is not in <store>, <fact>`.

#### Scenario: A manifest gone from the store

- **WHEN** a moon run passes `alpha:test` and `beta:test`, and
  `alpha:test`'s manifest is in neither the store the run's record
  names nor the checkout's own
- **THEN** `beta:test` is proven, and `alpha:test` is among the passes
  with no proof, its key that of `alpha:test`, saying
  `it passed, but nothing the run kept proves it ran: moon's manifest of
  it is not in <store>, where moon kept others of the run's manifests`
  (`a_pass_whose_manifest_is_gone_is_unkept_naming_the_store` in
  `crates/sluice/tests/profiles/moon.rs`)

#### Scenario: A dependency's manifest missing

- **WHEN** a passing task's own manifest is in the store and the
  manifest of a task it depends on is not
- **THEN** the task's proof is read as not kept, not as a key the tree
  does not bear out, naming the task whose manifest is missing and its
  key
  (`a_missing_manifest_is_told_from_one_the_tree_does_not_bear_out` in
  `crates/sluice/src/profiles/moon.rs`)

### Requirement: Only a pass with no proof is read so

A manifest the combination's tree does not bear out, a test that
failed, a replay and a task a check was failed by SHALL be read as
before, whether their manifests were kept or not (108, 139, 149, C.6).

#### Scenario: A key the tree does not bear out

- **WHEN** a run on `main`, whose state is on `origin`, passes
  `a:test` under a key whose manifest names inputs the tree does not
  hold
- **THEN** `a:test` is read as no pass under its key, as before, and
  #1 is blamed for it
  (`a_key_the_tree_does_not_bear_out_is_still_failed` in
  `crates/sluice-conformance/tests/unkept_proof.rs`)

#### Scenario: A failed test whose manifest is gone

- **WHEN** a moon run fails `gamma:test` and its manifest is not in the
  store
- **THEN** the check profile names `gamma:test` neither proven nor among
  the passes with no proof
  (`a_failed_task_whose_manifest_is_gone_is_not_unkept` in
  `crates/sluice/tests/profiles/moon.rs`)

### Requirement: A branch in one host's own files reads no proof

A branch whose state is in one host's own files SHALL read no proof of
its runs, and SHALL be read as before (78).

#### Scenario: Its runs keep no manifest

- **WHEN** `main`'s state is in the host's own files and a run of its
  combination keeps no proof of its passes
- **THEN** the combination passes and lands as before, and the log
  holds no `proof-missing` line
  (`a_branch_in_one_hosts_files_reads_no_proof` in
  `crates/sluice-conformance/tests/unkept_proof.rs`)

### Requirement: A missing proof is logged once per cause and host

When a run keeps no proof of a pass, the branch's log SHALL gain a
`proof-missing` line naming the host that read the run and why, once
for each distinct reason while it stands on that host, never once per
run. Each host's causes SHALL be logged apart (record 0105; rule
core.28).

#### Scenario: Two combinations on one host

- **WHEN** runs of two combinations on one host, three runs in all,
  each keep no proof of their passes in the same store
- **THEN** the log holds one `proof-missing` line for them
  (`the_cause_is_logged_once_across_runs_and_combinations` in
  `crates/sluice-conformance/tests/unkept_proof.rs`)

#### Scenario: Two hosts, and words that change

- **WHEN** runs on `build-1` and on `build-2` each keep no proof, and
  `build-1`'s later runs keep none in another store
- **THEN** the log holds a `proof-missing` line for `build-2` and one
  for each of `build-1`'s two stores
  (`a_cause_is_noted_once_per_host_and_reason` in
  `crates/sluice-core/src/proof.rs`)

### Requirement: A standing cause is kept in the branch's record

While a cause stands, `branch.json` SHALL keep it under
`proof_missing`, with its host, why, since when and how many of that
host's runs it touched. Nothing a round, gate, landing or submission
decides SHALL read it (rule core.14).

#### Scenario: Three runs touched

- **WHEN** three runs on `fake-host` kept no proof in the same store
- **THEN** `proof_missing` names `fake-host`, the reason and three runs
  (`the_cause_is_logged_once_across_runs_and_combinations`)

#### Scenario: The lines and the record read back

- **WHEN** the log holds a `proof-missing` and a `proof-kept` line
- **THEN** each reads back as written, and a `branch.json` written
  before `proof_missing` existed reads with none
  (`a_missing_proof_and_its_end_are_log_lines` in
  `crates/sluice-core/src/records.rs`)

### Requirement: The end of a cause is logged with how long and how many runs

When a run on a host whose cause stands proves at least one pass and
leaves none without proof, the branch's log SHALL gain a `proof-kept`
line for each of that host's causes, naming the host, why, when it
began, how many seconds it stood and how many runs it touched, and
`branch.json` SHALL keep it no longer. A run that passed nothing SHALL
end nothing (record 0105; rule core.28).

#### Scenario: The host keeps proof again

- **WHEN** `build-1`'s cause began at 09:16:40 and touched three runs,
  and its run at 09:46:40 proves its passes
- **THEN** `main`'s log gains
  `{"event":"proof-kept","host":"build-1","why":"<why>","since":"2026-11-09T09:16:40Z","stood":1800,"runs":3}`
  with that run's time, and `proof_missing` no longer names it
  (`a_cause_ends_with_how_long_and_how_many_runs` in
  `crates/sluice-core/src/proof.rs`)

#### Scenario: A run that passed nothing

- **WHEN** a run on `build-1` whose cause stands fails every test
- **THEN** the cause stands and nothing is logged
  (`a_run_that_proved_no_pass_ends_nothing` in
  `crates/sluice-core/src/proof.rs`)

#### Scenario: A cause that comes back

- **WHEN** a cause ended and a later run on the same host keeps no
  proof again
- **THEN** the log gains a new `proof-missing` line
  (`a_cause_ends_with_how_long_and_how_many_runs`)

### Requirement: moon's reason names whose manifest, which store and its fact

In moon's reason, `<whose>` SHALL be `it`, or `<task>, which it depends
on`, and `<store>` the store the run's record names. `<fact>` SHALL be
the first that holds of: `which does not exist`; `which <user> cannot
write`; `a directory of which <user> cannot write`; `where moon kept
none of the run's manifests`; `where moon kept others of the run's
manifests`. `<user>` is the user sluice runs as. The facts are read
from the store itself after the run, with nothing run (114; record
0105).

#### Scenario: What the store says of itself

- **WHEN** a manifest is missing from a store that does not exist; from
  one whose `blobs` its user cannot write; from one whose directory for
  the key its user cannot write; from one holding none of the run's
  manifests; and from one holding others of them
- **THEN** the fact read is, in turn, `which does not exist`, `which
  <user> cannot write`, `a directory of which <user> cannot write`,
  `where moon kept none of the run's manifests` and `where moon kept
  others of the run's manifests`
  (`the_store_says_why_a_manifest_is_missing` in
  `crates/sluice/src/profiles/moon.rs`)

### Requirement: A manifest is read where the run's moon kept it

The check profile SHALL read a key's manifest, and each manifest the
key depends on, in the store the run's record names. Where that store
is not the checkout's own `.moon/cache` and lacks a manifest, it SHALL
read the checkout's own store, before the checkout is released. A
manifest read from either SHALL be held to the same proof: it hashes to
its key, names its task, and names inputs the combination's tree holds
(C.2 moon row; rule core.9).

#### Scenario: moon kept the manifest in the checkout

- **WHEN** a moon run passes `alpha:test`, the run's record names a
  shared store, and `alpha:test`'s manifest is in the checkout's
  `.moon/cache` and not in the shared store
- **THEN** `alpha:test` is proven
  (`a_manifest_moon_kept_in_the_checkout_proves_its_key` in
  `crates/sluice/tests/profiles/moon.rs`)

#### Scenario: Neither store holds it

- **WHEN** neither store holds `alpha:test`'s manifest
- **THEN** `alpha:test` is among the passes with no proof, naming the
  store the run's record names
  (`a_manifest_moon_kept_in_the_checkout_proves_its_key`)

### Requirement: moon's dependency marks are no key

Where a manifest names a dependency `ignored`, `passthrough`, `outputs`
or `outputs-passthrough`, the marks moon 2.6.1 writes for a dependency
that gives the task no key, the check profile SHALL walk nothing for it
and SHALL NOT read the key as one the tree does not bear out. Any other
value that is not a key SHALL be refused as before (C.2 moon row, C.6).

#### Scenario: Each mark

- **WHEN** `b:test`'s manifest names `a:test` as a key, and `c:lint`,
  `d:gen`, `e:build` and `f:setup` as `ignored`, `passthrough`,
  `outputs` and `outputs-passthrough`
- **THEN** `b:test`'s key is borne out, walking `a:test` alone; and a
  dependency named `stale` is refused as no key
  (`moons_dependency_marks_are_no_key` in
  `crates/sluice/src/profiles/moon.rs`)

### Requirement: A task moon does not cache is proven

A task that moon runs and does not cache, `cache: false`, and each task
depending on it SHALL be proven by their manifests as any task is, on a
run that shares moon's cache in a linked worktree (C.2 moon row).

#### Scenario: An install and a check depending on it

- **WHEN** a run in a linked worktree with moon's shared worktree cache
  on runs `root:install`, with `cache: false` and no outputs, and
  `alpha:test`, which depends on it, with `--force`
- **THEN** both manifests are in the store the run's record names,
  `alpha:test`'s naming `root:install` as `ignored`, and both are proven
  (`a_task_moon_does_not_cache_and_its_dependent_are_proven` in
  `crates/sluice/tests/profiles/moon.rs`)

### Requirement: Each test left without proof names the missing key

A test that passed in a run that kept no proof of it SHALL be listed
not run with the key of the record that is missing, its own or, as its
reason names, a task's it depends on. The `lost` line and the JSON
under `lost` SHALL carry it as `key`. The reason SHALL carry no key, so
that its cause is logged once for each host and reason (record 0105;
rule core.28).

#### Scenario: The lost line

- **WHEN** a signed run passes `a:test` and `b:test` and keeps no proof
  of either
- **THEN** its `lost` line lists each with its key, the key the run
  gave it, and the log holds one `proof-missing` line
  (`a_lost_line_names_each_unkept_key` in
  `crates/sluice-conformance/tests/unkept_proof.rs`)

### Requirement: A key the tree does not bear out is blamed with why

Where a check's key is not borne out by the combination's tree for any
reason but a missing manifest, the submission blamed for it SHALL name
the check failed under that key with the check profile's words for
why, as `unsigned` in its record. A waiting submit and `sluice wait`
SHALL say, under the failed line,
`<task>: the combination's tree does not bear its key out: <why>`
(C.6, 20).

#### Scenario: A forged key

- **WHEN** a signed run passes `a:test` under a key whose record names
  an input the tree does not hold
- **THEN** the submission is blamed on `a:test` under its key, and its
  record says why under `unsigned`
  (`a_key_the_tree_does_not_bear_out_is_blamed_with_why` in
  `crates/sluice-conformance/tests/unkept_proof.rs`)

#### Scenario: The words

- **WHEN** `a:test` is blamed with the why `a:test: a/x is not in the
  tree`
- **THEN** the line reads `a:test: the combination's tree does not bear
  its key out: a:test: a/x is not in the tree`
  (`a_key_not_borne_out_is_said_with_why` in
  `crates/sluice/src/words.rs`)

#### Scenario: The check profile says why

- **WHEN** a moon run passes `alpha:test` under a key whose manifest
  names an input the tree does not hold
- **THEN** `proven` leaves `alpha:test` out and answers it among the
  refused with its key and the words `check_key` gave
  (`proven_drops_a_key_the_tree_does_not_bear_out` in
  `crates/sluice/tests/profiles/moon.rs`)
