## ADDED Requirements

### Requirement: A run held from the network is told it is offline

A sandboxed run whose policy's network is `denied` or `localhost` SHALL
start its command with `PROTO_OFFLINE=1`, so moon and the tools proto
runs make no name lookup to find out whether they are offline. A run
that may reach the network, and the equipping of a checkout, SHALL get
nothing (35, C.2 sandbox row).

#### Scenario: A run held from the network

- **WHEN** a sandboxed run's network is `localhost`
- **THEN** its command starts with `PROTO_OFFLINE=1`
  (`a_run_held_from_the_network_is_told_it_is_offline` in
  `crates/sluice/src/profiles/checks.rs`)

#### Scenario: A run that may reach the network

- **WHEN** a sandboxed run's network is `allowed`, or the run is open
- **THEN** its command gets no `PROTO_OFFLINE`
  (`a_run_held_from_the_network_is_told_it_is_offline`)
