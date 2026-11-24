## MODIFIED Requirements

### Requirement: The report is read from the records

sluice SHALL read a submission's report from its records alone: the submission, the branch's log and the results. The report SHALL read the same with no sluice process running, and after the task runner's own files are gone (26). It SHALL name the combination, the branch's revision that the combination extends, and each submission tested in it, in order.

#### Scenario: Read after the run's files are gone

- **WHEN** a submission was blamed, and moon's run report in the combination's checkout has since been removed
- **THEN** its report still names the combination, the branch revision and the submissions tested together, and lists every required check

#### Scenario: A landed submission

- **WHEN** a submission has landed and its record is among the branch's finished submissions
- **THEN** its report reads the combination it landed on

### Requirement: The report lists each required check

The report SHALL list each required check of the combination, on each system that the branch names. For each check, it SHALL give the check's name, its system, its key and one of these results:

- passed
- failed
- failed by a task that it depends on, which the report names, with no key of its own (149)
- flaky, with the result that it contradicts (19)
- could not run, with why and in how many runs in a row (114, 115)
- missing

#### Scenario: Two systems

- **WHEN** `main` needs `cli:build` on `linux-x64` and `macos-arm64`, and a combination passed it on `linux-x64` and failed it on `macos-arm64`
- **THEN** the report lists `cli:build` twice, passed on `linux-x64` and failed on `macos-arm64`

#### Scenario: Could not run

- **WHEN** `site:check` could not run in three runs in a row because it exited with a code the policy's `not_run` lists
- **THEN** the report lists `site:check` as could not run, naming the exit and the three runs, not as missing

#### Scenario: Flaky

- **WHEN** `app:e2e` failed on the combination and a pass for the same key is recorded, and the policy reads a flaky key as not passed
- **THEN** the report lists `app:e2e` as flaky, naming the pass and when it was recorded
