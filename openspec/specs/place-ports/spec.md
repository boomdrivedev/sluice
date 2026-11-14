# place-ports Specification

## Purpose
The ports a place holds on its host: each its own among every live
place on the host, taken as the lowest free run when the place is made,
recorded with the place, and read from that record ever after (31, 32;
record 0016 as amended 2026-11-13; rule core.10).

## Requirements

### Requirement: A place holds its ports themselves

The host SHALL hold each port a live place holds, by the port and
across every repository on it, and SHALL never give a port a live place
holds to another place, whatever their policies list (31, 32; record
0016 as amended 2026-11-13; rule core.10). A place SHALL take, from
`ports_from`, the lowest run of as many ports as its policy names in
which no port is held by a live place and each port is free.

#### Scenario: Two policies of different lengths

- **WHEN** repository A's policy names 21 ports, a place of A holds
  20147 to 20167, and a place is made in repository B, whose policy
  names 19
- **THEN** B's place holds 19 ports none of which is 20147 to 20167
  (`policies_of_different_lengths_hold_no_port_twice` in
  `crates/sluice/src/profiles/places.rs`)

#### Scenario: A gone place's ports are free again

- **WHEN** a place holds 20000 to 20002 and its record is removed
- **THEN** the next place made with three ports holds 20000 to 20002
  (`a_gone_places_ports_are_free_again` in
  `crates/sluice/src/profiles/places.rs`)

#### Scenario: A port in use is passed by

- **WHEN** no place holds 20001 but a process listens on it, and a place
  asks for two ports
- **THEN** it holds 20002 and 20003
  (`a_port_in_use_is_passed_by` in
  `crates/sluice/src/profiles/places.rs`)

### Requirement: A place's ports are fixed when it is made

A place's ports SHALL be recorded with the place when it is made, and
every later reading of them, its runs' `SLUICE_PORT_<NAME>`, its release,
a preview's names and a status, SHALL read that record and never reckon
them again from a policy. A change of the policy SHALL take effect for
places made after it. A release SHALL free each port its record names
(31; record 0016 as amended 2026-11-13).

#### Scenario: The policy changes under a session's place

- **WHEN** a worktree's place was given `SLUICE_PORT_API` 20004 at its
  first run, and its branch's policy then lists a port more before the
  name
- **THEN** its next run is given `SLUICE_PORT_API` 20004 again
  (`a_places_ports_are_read_from_its_record` in
  `crates/sluice/tests/profiles/places.rs`)

#### Scenario: A release frees what the record names

- **WHEN** a candidate's place holding three ports is released
- **THEN** none of the three is held on the host
  (`a_release_frees_the_ports_its_record_names` in
  `crates/sluice/tests/profiles/places.rs`)
