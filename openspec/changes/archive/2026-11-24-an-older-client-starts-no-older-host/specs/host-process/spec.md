## ADDED Requirements

### Requirement: A host process records the newest sluice that has run

Each host process SHALL record, as it starts, the newest sluice that has run as the host process of its state folder, in `host/newest`, with its version and its executable. It SHALL raise the record and never lower it. (`a-newer-command-replaces-an-older-host-process`)

#### Scenario: A host process starts

- **WHEN** a host process starts, and `host/newest` cannot be read
- **THEN** `host/newest` names the host process's version and its executable

### Requirement: A sluice older than the newest host process that has run starts no host process

A command, `sluice host` or `sluice daemon` of a sluice older than `host/newest` SHALL start no host process. It SHALL exit 3, and its words SHALL name its own version, the newer version, the sluice to run and the file to remove to run the older one. `sluice daemon` SHALL ask no host process to hand over first. (`a-newer-command-replaces-an-older-host-process`)

#### Scenario: An older command finds no host process answering

- **WHEN** `host/newest` names sluice 9.0.0, no host process answers, and an `overview --json` of an older sluice runs
- **THEN** it starts no host process, exits 3, and says that it starts no host process, that sluice 9.0.0 has run, that sluice 9.0.0 or newer is the one to run, and which file to remove

#### Scenario: An older `sluice host` or `sluice daemon` starts

- **WHEN** `host/newest` names sluice 9.0.0, and `sluice host` or `sluice daemon` of an older sluice starts
- **THEN** each exits 3 with the same words on its standard error, and the host's lock stays free

#### Scenario: An older daemon starts while a newer host process runs

- **WHEN** a host process runs, `host/newest` names sluice 9.0.0, and `sluice daemon` of an older sluice starts
- **THEN** the daemon asks the host process nothing and exits 3, and the host process answers with its pid and its `since` as before

### Requirement: A command older than the newest host process that has run asks for no hand-over

A command older than `host/newest` SHALL ask for no hand-over, and SHALL use the host process that answers as it is. `sluice hand-over` of such a sluice SHALL ask nothing and exit 3, with the words of a sluice that starts no host process. (`a-newer-command-replaces-an-older-host-process`)

#### Scenario: A command between the host process and the newest

- **WHEN** a host process of sluice 0.0.0 runs, `host/newest` names sluice 9.0.0, and a command of sluice 1.0.0 reaches the host process
- **THEN** it asks for no hand-over, and uses the host process with its pid and its `since` as before

#### Scenario: An older `sluice hand-over`

- **WHEN** a host process runs, `host/newest` names sluice 9.0.0, and `sluice hand-over` of an older sluice runs
- **THEN** it asks nothing and exits 3 with those words, and the host process answers with its pid and its `since` as before

### Requirement: A host process hands over to no sluice older than the newest that has run

A host process SHALL refuse a hand-over, and a daemon's ask to end, from a sluice older than `host/newest` or than itself. It SHALL answer with an error that names both versions, say so in its log, and go on as it was. A command whose hand-over gets an error SHALL fail at once with the error's words. (`a-newer-command-replaces-an-older-host-process`)

#### Scenario: An older sluice asks a host process to hand over

- **WHEN** `host/newest` names sluice 9.0.0, and a connection that says sluice 1.0.0 asks the host process to hand over, or to end for a daemon
- **THEN** the host process answers with an error that names sluice 1.0.0 and sluice 9.0.0, its log says so, and it answers with its pid and its `since` as before

#### Scenario: A newer command's hand-over is refused

- **WHEN** a command of sluice 0.72.0 asks a host process of sluice 0.71.0 to hand over, and the host process answers with an error
- **THEN** the command fails at once with the error's words, and waits for no hand-over
