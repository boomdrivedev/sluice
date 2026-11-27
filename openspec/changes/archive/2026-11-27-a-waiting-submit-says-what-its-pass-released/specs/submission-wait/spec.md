## ADDED Requirements

### Requirement: A waiting submit says what the pass that decided it released

A waiting `sluice submit` SHALL follow `sluice host` before it submits.
A step that ends its wait SHALL first let the passes in flight end. It
SHALL then name the last pass whose report went to the followers. The
submit SHALL say what each pass up to that one released before it
prints its end. This SHALL hold however soon the pass ended, and however
long the host held the command (122, 123).

#### Scenario: Held past its pass

- **WHEN** a host holds a waiting submit from the moment `sluice host`
  reads its submit
- **AND** it holds the submit until its pass tried the release of the
  checkout once
- **THEN** the submit says `could not release` with `(try 1 of 4)`,
  ends `passed · landed as <rev>`, and exits 0
  (`a_waiting_submit_held_past_its_pass_says_what_the_pass_released` in
  `crates/sluice/tests/landed.rs`)
