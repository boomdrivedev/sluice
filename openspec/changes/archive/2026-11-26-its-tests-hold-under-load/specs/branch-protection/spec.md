## ADDED Requirements

### Requirement: Writes of one hooks folder at one time each end well

`sluice host` SHALL write a hooks folder for one request at a time. A
`sluice protect` and a subscription that installs the hook at one time
SHALL each end well. The hook that was there before SHALL stay, chained
beside sluice's (124).

#### Scenario: A protect beside a subscription

- **WHEN** a hooks folder holds a hook of the person's, and eight
  installs of sluice's hook in it start at one time
- **THEN** each install ends well, the hook is sluice's, and the hook
  chained beside it is the person's
  (`installs_at_once_each_end_and_keep_the_hook_there` in
  `crates/sluice/src/protect.rs`)
