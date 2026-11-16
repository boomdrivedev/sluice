# check-systems Specification

## Purpose
Lets a branch require a check to pass on each system the repository
ships for: the policy names its systems, a branch names the systems each
required check needs, and a combination's result is read over every
check on every system named (85 to 87, 69).

## Requirements

### Requirement: The policy names its systems
The policy SHALL accept a `[systems]` table naming each system with its
`os` and `arch`, as the toolchain names them. A branch's `systems`,
`systems_for`, a signer's `systems` or a pool's runners naming a system
the table lacks SHALL be refused with a message naming it.

#### Scenario: An unknown system
- **WHEN** `[branches.main] systems = ["linux-riscv"]` and `[systems]` has no `linux-riscv`
- **THEN** the policy is refused and the message names `linux-riscv`

### Requirement: Every result records its system
Every recorded result SHALL name the system it ran on. One check run on
two systems SHALL be two results, and a pass on one system SHALL never
stand for another (85, 69).

#### Scenario: One key on two systems
- **WHEN** `cli:build` passes on `linux-x64` and has never run on `macos-arm64`
- **THEN** a gate that needs `cli:build` on `macos-arm64` reads it as missing there

### Requirement: A branch names the systems each check needs
A branch's table SHALL accept `systems`, the systems every required
check needs, and `[branches."<pattern>".systems_for]`, the systems one
check needs in place of those. A combination SHALL pass only when every
required check passes on every system named for it. A check with none
named SHALL take one pass from any system (86).

#### Scenario: A build on three systems
- **WHEN** `main` has `systems = ["linux-x64"]` and `systems_for."cli:build" = ["linux-x64", "linux-arm64", "macos-arm64"]`
- **THEN** a combination passes only when `cli:build` passed on all three and every other required check passed on `linux-x64`

#### Scenario: No systems named
- **WHEN** `bolt/*` names no systems and `#check-fast` passed on `macos-arm64` alone
- **THEN** `#check-fast` reads as passed for a combination of a bolt

### Requirement: A combination opens one run per system
The lead SHALL open, for each combination, one run per system some
required check needs, holding the checks that need that system. Checks
needing no system SHALL go in one of those runs, or in a run any system
may take when there is none (86).

#### Scenario: Runs of one combination
- **WHEN** a combination of `main` is opened under the policy above
- **THEN** three runs are open: `linux-x64` with every required check, `linux-arm64` and `macos-arm64` with `cli:build` alone

### Requirement: A run is taken only on its system
A run SHALL be claimed only by a host whose system it is and whose
signer may claim that system, or started on a pool that has a runner for
it. A run that no live host or pool can take SHALL stay open, and the
combination's `timeout` SHALL read it as not passed (66, 87).

#### Scenario: Nobody runs the system
- **WHEN** a run needs `macos-arm64` and no host or pool lists it
- **THEN** the run stays open, and when the branch's `timeout` passes the combination reads as failed with `cli:build` on `macos-arm64` named as missing

### Requirement: The CLI shows a result per check per system
`sluice status`, `sluice overview` and a blamed submission's report SHALL
name the system of every result they show, and a failure SHALL name the
check and the system it failed on.

#### Scenario: A failure on one system
- **WHEN** `cli:build` fails on `linux-arm64` only
- **THEN** the blamed submission's report names `cli:build` on `linux-arm64` and the place its output is kept
