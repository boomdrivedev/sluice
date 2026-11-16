## MODIFIED Requirements

### Requirement: A combination whose passes had no proof is tested again

A combination whose run kept no proof of its passes SHALL test those
checks again in a follow-up run that holds them alone, every other
check of the run keeping its outcome (run-life), and the run SHALL count
toward the runs in a row whose tests could not run, so that the third
such run reads the combination as not passed and blames with why (114,
115).

#### Scenario: The next run keeps its proof

- **WHEN** the combination's follow-up run passes both tests and keeps
  their proof
- **THEN** both passes are recorded under their keys and #1 lands
  (`a_pass_nothing_proves_has_no_outcome_and_runs_again`)

#### Scenario: Three runs in a row

- **WHEN** three runs in a row of the same combination, a run and its
  follow-ups, each keep no proof of its passes
- **THEN** the third leaves the combination not passed, its newest
  submission blamed, and its session told each test could not run, with
  why (`three_runs_whose_passes_nothing_proves_blame_with_why` in
  `crates/sluice-conformance/tests/unkept_proof.rs`)
