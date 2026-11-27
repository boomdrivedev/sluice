## ADDED Requirements

### Requirement: A test names a run's check by the leader of its group

A test of sluice that finds a run's check by its command line SHALL
name the process that leads its process group. The run's supervisor
starts the check in a group of its own, so the check leads that group.
A copy that the check's shell forked SHALL NOT be named, because it is
in the group of the shell. So a test that reads the parent of the
check reads the run's supervisor, whatever the timing of the forks of
the shell.

#### Scenario: A check's shell between a fork and an exec

- **WHEN** a check's shell holds a copy of itself that it forked, and
  the copy has the command line of the shell
- **THEN** each read of the test names the shell, which leads the
  group, and does not name the copy
