# Spec Delta

## ADDED Requirements

### Requirement: A step on a host that does not run a branch's queue says another host runs it

A `sluice step` of a branch whose state is on its remote, while another
host runs its queue and that host's lease has not passed on this host,
SHALL say `elsewhere`: as its line, `<branch>: elsewhere: another host
runs the branch's queue, and its lease has not passed`, and as the
document's `outcome` under `--json`. Nothing the step prints SHALL say
`led` (72, 73, 75; core.26).

#### Scenario: Host B steps a branch whose queue host A runs

- **WHEN** host A runs `main`'s queue on origin and wrote its state
  within the lease, and host B runs `sluice step main`, then `sluice
  step main --json`
- **THEN** B prints `main: elsewhere: another host runs the branch's
  queue, and its lease has not passed`, the document's `outcome` is
  `elsewhere`, and neither says `led`
  (`step_on_a_host_that_does_not_run_the_queue_says_elsewhere` in
  `crates/sluice/tests/cli.rs`)
