## ADDED Requirements

### Requirement: A test that fails in a run the sandbox refused something is a fail

After a run in which a test did not pass and its time limit did not
end, sluice SHALL read what the sandbox refused the run. A test that
failed, or was failed by a task it depends on, SHALL keep its result:
the fail is recorded under its key and its entry is blamed. What the
sandbox refused SHALL be said beside the fail, on the run's `detail`
line in the branch's log, in `sluice status` and in `sluice exec`. A
pass SHALL stand (139, 149).

#### Scenario: A refused run

- **WHEN** a run of `main`'s combination has `a:test`, which fails, and
  `b:test`, which passes, and the sandbox reports refusing the run
  `network-outbound /private/var/run/mDNSResponder`
- **THEN** `b:test`'s pass and `a:test`'s fail are recorded under their
  keys, the submission is blamed after that one run, and the run's
  `detail` line says sluice's sandbox refused the run a name lookup
  (`a_check_failed_in_a_refused_run_is_a_fail_that_names_the_refusal`
  in `crates/sluice-conformance/tests/could_not_run.rs`)

#### Scenario: A test failed by a task the run was refused

- **WHEN** a required test is skipped for a task it depends on that
  failed, in a run the sandbox reports refusing something
- **THEN** the task's fail is recorded under its key, the test is
  failed by it, and the submission is blamed
  (`a_refusal_leaves_a_check_failed_by_a_dependency_failed_by_it` in
  `crates/sluice-conformance/tests/failed_dependency.rs`)

### Requirement: A switch and an engine gone give one reason

After a run in which a test did not pass, sluice SHALL ask first whether
the host's system was switched, then what the sandbox refused, then,
where the policy asks for one, whether the container engine has gone. A
switch SHALL be the reason, and nothing after it SHALL be asked. An
engine gone SHALL be the reason where the system was not switched,
whatever the sandbox refused (84, 114, 139; records 0074, 0097).

#### Scenario: A switch and a refusal

- **WHEN** a test fails in a run during which the host's system was
  switched and the sandbox refused something
- **THEN** the test is not run, saying the host's system was switched,
  and what the sandbox refused is not read
  (`a_switch_and_a_refusal_name_the_switch` in
  `crates/sluice-conformance/tests/could_not_run.rs`)

#### Scenario: A refusal and an engine gone

- **WHEN** `main`'s policy says `docker = true`, a test fails, the
  sandbox refused the run something, and the container engine no longer
  answers after it
- **THEN** the test is not run, saying the container engine no longer
  answers, and nobody is blamed
  (`a_refusal_and_an_engine_gone_name_the_engine`)

## MODIFIED Requirements

### Requirement: sluice exec reads a refusal as a combination's run does

A session's run of the branch's tests SHALL read what the sandbox
refused it as a combination's run does: a fail is recorded, and what
was refused is said after the run's lines. A command after `--` SHALL
never be read so (92, 139; record 0074).

#### Scenario: A session's run refused something

- **WHEN** a session's run of the branch's tests has a test that fails
  and the sandbox refused the run something
- **THEN** the fail is recorded under its key, and the run's report
  names what was refused
  (`a_session_run_refused_something_records_its_fail` in
  `crates/sluice-conformance/tests/could_not_run.rs`)

#### Scenario: S66

- **WHEN** `sluice exec` runs a test that writes a folder the host does
  not declare, and then runs it again once the host's `[[caches]]`
  declares that folder
- **THEN** the first run records the fail and names the write the
  sandbox refused; the second records its pass under a wider grant,
  which no reading weighs with the fail
  (`exec_records_a_fail_the_sandbox_refused_with_its_words_and_its_pass_under_a_wider_grant`
  in `crates/sluice/tests/exec.rs`)

## REMOVED Requirements

### Requirement: A test that did not pass in a run the sandbox refused something records no result

**Reason**: A refusal in every run of the gate erased every fail. sluice
ran each failing combination three times and blamed nobody for a real
failure.

**Migration**: "A test that fails in a run the sandbox refused something
is a fail". A refusal that makes a test fail is fixed at its cause: the
policy, the host's `[[caches]]` or the test.

### Requirement: A switch, a refusal and an engine gone give one reason

**Reason**: A refusal no longer gives a reason.

**Migration**: "A switch and an engine gone give one reason".
