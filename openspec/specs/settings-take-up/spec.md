# settings-take-up Specification

## Purpose
How a `sluice run` kept running takes up a change of the host's own
settings, `~/.config/sluice/config.toml`, between its passes and never
within one, so nothing on the host restarts it (135; record 0107; rule
core.54). It tells a change by the file's sign alone, keeps what it
holds where the file does not read or the settings do not bind, said
once, and leaves a run already started on what it started with.

## Requirements

### Requirement: A look at a file that stands reads nothing

Between passes, the host process and a kept `sluice run` SHALL tell
whether the settings file changed from the file's sign alone, with no
process started and no git read (rule core.31). Each SHALL read the file
only once the sign differs from the one taken as its settings were last
read. A read during which the sign moved SHALL count as no read, and the
file SHALL be read again at the next look.

#### Scenario: An unchanged file made unreadable

- **WHEN** the settings held are the file's, and the file's permissions
  are then taken away with its contents, time and size unchanged
- **THEN** the next look finds nothing changed, and nothing is said
  (`a_look_at_a_file_that_stands_reads_nothing` in
  `crates/sluice/src/host_settings.rs`)

### Requirement: The sign takes in a link and the file it reaches

The file's sign SHALL be the modification time and size of the file's
own entry, the path the file resolves to through every link, and the
modification time and size of the file found there, so a link moved
into a store whose files all share one time is seen.

#### Scenario: A link moved to a store file of the same size and time

- **WHEN** the settings file is a link to a file whose modification time
  is one second past the epoch, as Nix's store gives every file, and the
  link is moved to another such file of the same size holding other
  settings
- **THEN** the next look finds it changed and gives the other settings
  (`a_link_moved_to_a_file_of_the_same_size_and_time_is_a_change` in
  `crates/sluice/src/host_settings.rs`)

### Requirement: A file that does not read leaves the host process on what it holds

Where the changed settings file is gone, cannot be read, or holds
settings sluice refuses, the host process SHALL keep the settings it
holds, and SHALL say in its log, once for that change, the file and why.
A gone file SHALL be read as one that does not read, never as sluice's
defaults. It SHALL read the file again once it changes again, and SHALL
never end for it (record 0107; rule core.54).

#### Scenario: A file sluice refuses, and then mends

- **WHEN** a bound of two is held, and the file is changed to `[runs]
  max = "two"`, then looked at again with no change, and then changed to
  `max = 3`
- **THEN** the first look says the file is refused, naming it and why,
  and keeps a bound of two; the second finds nothing changed and says
  nothing; and the third gives a bound of three
  (`a_file_that_does_not_read_leaves_the_settings_held_and_is_said_once`
  in `crates/sluice/src/host_settings.rs`)

#### Scenario: A gone file

- **WHEN** the file `[runs] max = 3` was taken up from is removed
- **THEN** the next look says the file is gone and that an empty file
  gives sluice's defaults, and keeps a bound of three
  (`a_file_that_does_not_read_leaves_the_settings_held_and_is_said_once`
  in `crates/sluice/src/host_settings.rs`)

### Requirement: Settings that do not bind leave the host process on what it holds

Where the changed file reads but its settings cannot be bound, such as
a key that does not parse or systems a branch's policy gives otherwise,
the host process SHALL keep the settings and the binding it holds, and
SHALL say in its log, once for that change, the file and why (rule
core.54).

#### Scenario: A key that holds no private key

- **WHEN** the host's settings say `[runs] max = 1`, the host process
  runs with two submissions queued, and the file is changed to `max = 2`
  with a `[forge]` table whose key, mode `0600`, holds no private key
- **THEN** the second combination's run starts only once the first has
  ended, given the cores the first was, and both submissions land
- **AND** the host process's log says once that the file is refused and
  it keeps the settings it holds, naming the file and the key
  (`the_host_process_keeps_its_settings_when_the_new_ones_do_not_bind`
  in `crates/sluice/tests/instance.rs`)

### Requirement: A run keeps the settings it started under

A run that started before the host process took up changed settings,
or handed its repositories over, SHALL go on with the cores, caches and
grant it was started with. No run SHALL be stopped or started again
because the settings changed, and its result SHALL count as it would
have (rule core.54).

#### Scenario: The first run keeps its cores

- **WHEN** the bound grows from one to two while the first combination's
  run is going
- **THEN** that run was given all the cores sluice may use, keeps them
  to its end, is not started again, and its submission lands
  (`the_host_process_takes_up_a_changed_bound_at_its_next_pass` in
  `crates/sluice/tests/instance.rs`)

### Requirement: The host process keeps its repositories across a take-up

The host process SHALL take up changed settings with every repository
it holds kept: each task SHALL keep its reader's lock, its followers and
the parts its rounds hold, and SHALL bind afresh at the start of its
next pass, reading each policy once more.

#### Scenario: A pass under a new binding

- **WHEN** the host process has passed `main` once with #1 queued, a
  watch follows it, and its next passes run under a binding made afresh
- **THEN** the reader's lock is held throughout, the watch prints on, and
  #1 lands
  (`the_host_process_keeps_its_repositories_across_a_new_binding` in
  `crates/sluice/tests/host_process.rs`)

### Requirement: The run started afresh is the run as it was started

The process a kept `sluice run` starts afresh SHALL be the same process,
keeping its id, its output and whatever runs it. It SHALL run the
executable the run started as, resolved through every link as the run
started, with the command line the run was given, the folder it ran in,
and the environment it started with before telemetry took anything out
of it (151; rule core.53).

#### Scenario: The hand-over's command

- **WHEN** a run that started as `/x/sluice` with `run --rounds 12` and
  an environment holding `OTEL_EXPORTER_OTLP_ENDPOINT` hands over, with
  that variable since taken out of its own environment
- **THEN** the command it starts afresh is `/x/sluice` with `run
  --rounds 12`. Its environment is cleared and set to exactly the one
  it started with, the variable among it, and the hand-over's document.
  That document, holding settings, tables, the passes followed and what
  was said, reads back as it was written
  (`the_run_started_afresh_is_the_one_that_started` in
  `crates/sluice/src/hand_over.rs`)

### Requirement: The run started afresh starts on the settings handed over

The run started afresh SHALL start on the settings the run handed over,
never on a reading of the file made as it starts. Its first look SHALL
read the file once. Settings equal to those handed over SHALL be taken
up silently. A change made meanwhile SHALL be taken up as any change
is. A file that does not read SHALL leave it on the settings handed
over, said once (rule core.54).

#### Scenario: The first look after a hand-over

- **WHEN** a run holds `[runs] max = 3` handed over, and the file holds
  `max = 3`, then `max = 4`, then `max = "two"`
- **THEN** the first look finds nothing to take up, the second finds a
  bound of four, and the third refuses the file naming `max` and holds
  a bound of four
  (`a_run_handed_over_holds_what_it_was_handed_and_reads_the_file_once`
  in `crates/sluice/src/host_settings.rs`)

### Requirement: The host process takes up changed host settings before its next pass

The host process, running when the host's settings file changes, SHALL
take up the settings the file then holds between its passes and never
within one: each repository's next pass, and the room's next admission,
act under the new settings, and no pass acts under two. Nothing on the
host SHALL need to restart it for a change to count (135; record 0107;
rule core.54).

#### Scenario: The bound grows while the host process runs

- **WHEN** the host's settings say `[runs] max = 1`, the host process
  runs with two submissions queued on `bolt/a`, whose policy tests four
  at once, and the file is changed to `max = 2` while the first
  combination's run is going
- **THEN** the second combination's run starts while the first is still
  going, and is given the cores sluice may use divided by two, and both
  submissions land
  (`the_host_process_takes_up_a_changed_bound_at_its_next_pass` in
  `crates/sluice/tests/instance.rs`)

#### Scenario: A file written after the host process started

- **WHEN** the host process started with no settings file, and so with
  every default, and a file naming `[runs] max = 3` is then written
- **THEN** the next look finds it changed and reads it, and the settings
  it gives have a bound of three
  (`a_changed_file_is_read_once_and_the_same_settings_change_nothing` in
  `crates/sluice/src/host_settings.rs`)

#### Scenario: The same settings written again

- **WHEN** the file is written again with the settings held, as an apply
  of the host's configuration that changed another tool's file does
- **THEN** the next look finds nothing to take up
  (`a_changed_file_is_read_once_and_the_same_settings_change_nothing` in
  `crates/sluice/src/host_settings.rs`)

### Requirement: The host process says which tables a take-up changed

For each change it takes up, the host process SHALL say once in its log
the file and the tables whose settings changed, in the order the
configuration reference gives them: `[runs]`, `[forge]`, `[signing]`,
`[peers]`, `[deliveries]`, `[telemetry]`, `[[caches]]`. It SHALL never
say it took up a change that names no table. Settings equal to those it
holds SHALL be taken up silently, with nothing bound afresh.

#### Scenario: The host process says what it took up

- **WHEN** the bound grows from one to two while the host process runs
- **THEN** its log says exactly once `sluice: took up the host's
  settings in <path>: [runs] changed`
  (`the_host_process_takes_up_a_changed_bound_at_its_next_pass` in
  `crates/sluice/tests/instance.rs`)

#### Scenario: Which tables changed

- **WHEN** the settings taken up differ from those held in `[runs]
  max` and `[forge] discovery` alone
- **THEN** the tables named are `[runs]` and `[forge]`, in that order
  (`the_tables_a_change_moves_are_named` in
  `crates/sluice/src/instance.rs`)

#### Scenario: A changed telemetry table is named

- **WHEN** the settings taken up differ from those held in
  `[telemetry] collector` and `[[caches]]`, or in `[telemetry] logs`
  alone
- **THEN** the tables named are `[telemetry]` and `[[caches]]`, in that
  order, or `[telemetry]` alone
  (`a_changed_telemetry_table_is_named_and_taken_up_afresh` in
  `crates/sluice/src/instance.rs`)

### Requirement: A change no binding takes up in place goes to a fresh host process

Where the tables a change moves include one no binding takes up in
place, `[telemetry]` alone of today's, the host process SHALL bind the
new settings and check the host's systems under them, send what its
telemetry holds, let each task finish its pass, and start a fresh host
process of its own, which reads the file itself. Nothing SHALL pass
between the two but what the records and the host's state hold (135;
record 0107 as amended 2026-11-13; rule core.54).

#### Scenario: Which tables hand over

- **WHEN** a change moves `[telemetry]`, alone or with other tables, or
  moves `[runs]` and `[forge]` alone
- **THEN** the first hands over and the second is taken up in place
  (`a_changed_telemetry_table_is_named_and_taken_up_afresh` in
  `crates/sluice/src/instance.rs`)

#### Scenario: The host process sends to the collector it is changed to

- **WHEN** the host's settings say `[runs] max = 1` and `[telemetry]`
  with `send = true` and one collector, the host process runs with two
  submissions queued on `bolt/a`, and the file is changed to name another
  collector while the first combination's run is going
- **THEN** the first collector is sent traces and metrics, the second is
  sent traces, both submissions land, and a host process of another pid
  answers
- **AND** each combination's run started once, given every core sluice
  may use
  (`the_host_process_hands_a_changed_telemetry_table_to_a_fresh_one` in
  `crates/sluice/tests/instance.rs`)

### Requirement: A kept sluice run counts its passes across a hand-over

A kept `sluice run`, which follows the host process, that starts itself
afresh SHALL count the passes it already followed toward `--rounds`, and
SHALL not say again what it said before.

#### Scenario: A run handed over counts its passes

- **WHEN** a `sluice run --rounds 3` over `main` follows one pass, hands
  over what it carries, and follows on from what it handed over
- **THEN** it ends after two passes more
  (`a_run_handed_over_keeps_following_and_counts_its_passes` in
  `crates/sluice/tests/cli.rs`)

### Requirement: A kept sluice run whose start afresh fails keeps what it holds

Where a kept `sluice run`'s settings do not bind, or its process cannot
start afresh, it SHALL keep the settings and the telemetry it holds. It
SHALL say on stderr once for that change the file, the tables, and why.
It SHALL read the file again once it changes again, and SHALL never end
for it (rule core.54).

#### Scenario: An executable that cannot start

- **WHEN** a hand-over's command names an executable that is not there
- **THEN** starting it answers the system's words, naming the executable
  (`a_hand_over_that_cannot_start_says_why` in
  `crates/sluice/src/hand_over.rs`)

#### Scenario: A start that fails leaves the process as it was

- **WHEN** a hand-over's start fails while another thread of the
  process reads its environment
- **THEN** that thread never reads the environment handed over, and
  `SIGPIPE` keeps its disposition
  (`a_hand_over_that_cannot_start_leaves_the_process_as_it_was` in
  `crates/sluice/src/hand_over.rs`)

### Requirement: Work in play never waits for a command across the host process's hand-over

The host process SHALL start its successor itself, holding the host's
start lock from before it lets its own lock go until a host process of
another pid answers, so a command that meets neither waits on the start
lock and reaches the successor, and work in play never waits for another
command to start a host process (135; record 0107 as amended
2026-11-13).

#### Scenario: A submit during the hand-over

- **WHEN** the host process is handing over for a changed `[telemetry]`
  and a submit runs on the host meanwhile
- **THEN** the submit reaches the successor, exactly one host process
  runs once the hand-over ends, and the submission lands with no other
  command run
  (`a_submit_during_a_hand_over_reaches_the_successor` in
  `crates/sluice/tests/host_process.rs`)

### Requirement: A successor that does not start leaves the host process as it was

Where the host process's settings for a change it hands over for do not
bind, or no successor answers within a start's ten seconds, it SHALL
keep its repositories, the settings, binding and telemetry it holds, go
on passing, and say once in its log the file, the tables and why. It
SHALL read the file again once it changes again, and SHALL never end for
it (rule core.54).

#### Scenario: Settings it would hand over for that do not bind

- **WHEN** the host's settings say `[runs] max = 1`, the host process
  runs with two submissions queued on `bolt/a`, and the file is changed
  in `[runs]`, `[forge]` and `[telemetry]`, with a `[forge]` key that
  holds no private key, while the first combination's run is going
- **THEN** the second combination's run starts only once the first has
  ended, both submissions land, and the same host process runs throughout
- **AND** its log says exactly once `sluice: warning: <path> changed
  [runs], [forge], [telemetry], which the host process takes up only by
  handing over, and its settings do not bind, so it keeps the settings it
  holds until the file changes: <why>`, naming the key
  (`the_host_process_keeps_its_settings_when_a_change_it_hands_over_for_does_not_bind`
  in `crates/sluice/tests/instance.rs`)

#### Scenario: A successor that does not answer

- **WHEN** the host process hands over and the successor it starts exits
  at once
- **THEN** it takes its lock back, its socket answers with its own pid,
  and its log says once that its successor did not start
  (`a_successor_that_does_not_start_leaves_the_host_process_as_it_was`
  in `crates/sluice/tests/host_process.rs`)
