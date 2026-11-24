## REMOVED Requirements

### Requirement: The operator may set aside a recorded result

**Reason**: By the user's answer of 2026-11-23, the set-aside goes: its command, its records and its reads. A fail that the host caused is a fault of sluice. sluice is fixed so that such a run records no outcome, as records 0031, 0043 and 0072 do.

**Migration**: A set-aside that an earlier release wrote stays in the records, and no reader reads it. A key that holds a set-aside fail and a pass reads flaky until a real fix changes what the check reads.

## ADDED Requirements

### Requirement: A result is never set aside

sluice SHALL have no command that sets a result aside. It SHALL write no set-aside and SHALL read none. Each branch SHALL read each recorded result.

Before: the operator set a recorded result aside, and no branch read it after that. Now: a fail that the host caused is a fault of sluice. sluice is fixed so that such a run records no outcome, as records 0031, 0043 and 0072 do.

#### Scenario: set-aside is no command

- **WHEN** a person runs `sluice set-aside cli:tools --at 2026-10-17T13:05:32Z --reason x`
- **THEN** sluice says that `set-aside` is no command, exits 64, as for any command that sluice does not know, and writes nothing

#### Scenario: A set-aside that an earlier release wrote

- **WHEN** a branch's records hold a set-aside of a fail of `cli:tools` that an earlier release wrote, and the key also holds a pass
- **THEN** the key reads flaky, and the branch's status names the fail and the pass

## MODIFIED Requirements

### Requirement: A failure says what to do without changing inputs

When a submission fails on a test read as flaky, sluice SHALL name the result that the fail contradicts. It SHALL say that the same head lands once a real fix changes what the test reads. It SHALL say that a fail the host caused is a fault of sluice. It SHALL NOT ask for a change to what the test reads. For a submission that timed out, it SHALL name each test that the limit ended. It SHALL say that it recorded nothing for the test, and that the head may be submitted again.

#### Scenario: A flaky failure

- **WHEN** a submission fails with `a:test (flaky)` and the other result is a fail
- **THEN** the output names the other result and when it was recorded
- **AND** it says that the same head lands once a real fix changes what `a:test` reads
- **AND** it says that a fail the host caused is a fault of sluice
- **AND** the output names no command that sets a result aside, and has no line that asks for a change to what `a:test` reads

#### Scenario: A timed-out submission

- **WHEN** a submission timed out while `b:test` ran
- **THEN** the output says `b:test` was running when the time limit ended
  the run, gives its log, and says nothing was recorded for it
