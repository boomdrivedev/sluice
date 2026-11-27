## ADDED Requirements

### Requirement: A test does not open files again and again in a folder that sluice host watches

A test of sluice SHALL NOT open a file again and again in a folder that `sluice host` watches. On Linux, each open is a file event. notify adds a watch only after it has read all the events that came. So events that do not stop hold back each new watch of `sluice host`. A test that asks for a lock again and again SHALL ask on one descriptor that it opened one time.

#### Scenario: A daemon that starts afresh while a test asks for the host's lock

- **WHEN** a daemon starts afresh in place on Linux, and a test asks for the host's lock all through the start
- **THEN** the image started afresh answers, and the test never gets the lock (`a_daemon_starting_afresh_in_place_keeps_the_hosts_lock` in `crates/sluice/tests/host_process.rs`)
