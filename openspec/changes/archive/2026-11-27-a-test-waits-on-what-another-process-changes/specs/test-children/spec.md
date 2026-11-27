## ADDED Requirements

### Requirement: A test waits on a state that another process changes

Another process can change a state that a test of sluice reads. The test SHALL read that state until it is the state that the test expects, up to a limit. It SHALL NOT read it one time and expect it. A test SHALL read such a state one time for each look, and SHALL keep the state that it read.

#### Scenario: A landing that another host made

- **WHEN** a second host lands a first host's submission on the remote
- **THEN** the test reads the submission's state on the first host until it is `landed`, for one minute at most (`a_host_whose_own_main_is_refused_counts_the_remotes_signers` in `crates/sluice/tests/peers.rs`)

#### Scenario: A source that the watcher starts again

- **WHEN** a test kills the watcher's source
- **AND** the killed source runs as a zombie until the watcher reaps it
- **THEN** each look of the test reads the running sources one time
- **AND** the new source is the one source of that read (`watch: source exits` in `plugins/herdr/tests/run.sh`)
