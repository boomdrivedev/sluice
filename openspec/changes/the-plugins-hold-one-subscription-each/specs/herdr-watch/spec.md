## ADDED Requirements

### Requirement: The watcher holds one subscription for every repository

The watcher SHALL run the default source one time for all repositories. For each repository with a pane of a server that it serves, the watcher SHALL write one `add <folder>` line on the source's input. The folder SHALL be the top of a worktree of that repository with a pane in it. The source's input SHALL stay open while the source runs. (`one-subscription-per-plugin@ba0cdba772ca`)

#### Scenario: Three repositories

- **WHEN** panes are open in three repositories with sluice's records, and there is no `sources.toml`
- **THEN** one `sluice overview --json --watch --stdin` runs
- **AND** its input holds one `add` line for each of the three repositories

#### Scenario: Two worktrees of one repository

- **WHEN** two panes are open in two worktrees of one repository with sluice's records
- **THEN** the source's input holds one `add` line for that repository

#### Scenario: A pane in a new repository

- **WHEN** the source serves one repository, and a pane opens in a second repository with sluice's records
- **THEN** the watcher writes `add` for the second repository on the same input
- **AND** no second source starts

### Requirement: The watcher removes the folder of a repository with no pane

When no pane of a server is in a repository, the watcher SHALL write `remove <folder>` on the source's input. The folder SHALL be the folder that the watcher added for that repository. The source SHALL continue to run for the other repositories.

#### Scenario: A pane closes in one of two repositories

- **WHEN** the source serves two repositories, and the last pane in the first repository closes
- **THEN** the watcher writes `remove` with the folder that it added for the first repository
- **AND** the source still runs, and the popup still holds the second repository

### Requirement: A source that starts again gets each folder again

When the source ends, the watcher SHALL start it again within one tick. The watcher SHALL then write `add` again for each repository that it holds.

#### Scenario: The source ends

- **WHEN** the source serves two repositories, and the source's command exits
- **THEN** within one tick a new source runs
- **AND** its input holds one `add` line for each of the two repositories

#### Scenario: The watcher is killed

- **WHEN** the watcher is killed with `SIGKILL` while its source serves a repository, and a pane event starts a new watcher
- **THEN** within two ticks the killed watcher's source has ended, and one source runs

### Requirement: Each document names its folder

The reader of the source SHALL keep each document under its own `host` and `repository`. To learn a submission's worktree or its blamed tests, the reader SHALL run `sluice status` in the `folder` that the document's line names.

#### Scenario: A blame in the second repository

- **WHEN** the source serves two repositories, and a line with `folder` set to the second repository's worktree shows `#9` blamed
- **THEN** the reader runs `sluice status 9 --branch <branch> --json` in that worktree

### Requirement: The watcher reads a repository's records only from its subscription

The watcher SHALL NOT run `sluice overview --json` to find whether a repository has sluice's records. A document that holds a branch or a session SHALL mark its repository as having records, and the watcher SHALL keep it. (`reads-go-through-the-resident@69134e5825d7`)

#### Scenario: A repository with records

- **WHEN** a pane opens in a repository whose first document holds `main` with `#3` queued
- **THEN** the popup opened from that pane shows `#3` queued
- **AND** the watcher ran no `sluice overview --json`

### Requirement: A repository with no sluice is added again at a later look

A document with `"set_up": false` or `"gone": true` SHALL mark its repository as having no records. The watcher SHALL keep no document of it. The watcher SHALL write `add` for that repository again at a look five minutes after the mark, or at a look after a nudge.

#### Scenario: A repository with no sluice

- **WHEN** a pane is open in a repository whose document says `"set_up": false`
- **THEN** the popup opened from that pane shows `No sluice records in this repository.`

#### Scenario: Asked again after five minutes

- **WHEN** a repository's document said `"set_up": false`, sluice is then set up there, and a look comes five minutes later
- **THEN** the watcher writes `add` for the repository again, and keeps the document that comes

### Requirement: A repository with no records stays on the subscription

A document that holds no branch and no session SHALL mark its repository as having no records. The watcher SHALL keep no document of it. The repository's folder SHALL stay on the subscription.

#### Scenario: The first submission

- **WHEN** a repository with sluice and no records is on the subscription, and a submission is then queued there
- **THEN** the next document of the repository is kept, and the popup shows the submission queued, with no wait of five minutes

### Requirement: A sluice with no --stdin is named

The watcher SHALL keep the source's standard error. When the source ends with `unexpected argument '--stdin'` or `unrecognized subcommand` in its standard error, the popup SHALL show one line: `<sluice> cannot serve the plugin's folders on one subscription: the plugin needs sluice <version>`. `<sluice>` SHALL be the first line of `sluice --version`. `<version>` SHALL be the version in `herdr-plugin.toml`. The next document that a source prints SHALL clear the line.

#### Scenario: An older sluice on the host

- **WHEN** the source exits 2 and writes `error: unexpected argument '--stdin' found` to its standard error, `sluice --version` prints `sluice 0.97.5`, and `herdr-plugin.toml` names `0.98.0`
- **THEN** the popup shows `sluice 0.97.5 cannot serve the plugin's folders on one subscription: the plugin needs sluice 0.98.0`

### Requirement: A source with folders = true gets the folders on its input

A source in `sources.toml` with `folders = true` SHALL run one time, and SHALL get the `add` and `remove` lines on its input. A source without `folders = true` SHALL run one time with its input closed.

#### Scenario: A source of the user's that names its folders

- **WHEN** `sources.toml` names a source `local` with command `["sluice", "overview", "--json", "--watch", "--stdin"]` and `folders = true`, and a pane is open in a repository with sluice's records
- **THEN** that command runs one time, and its input holds one `add` line for that repository

#### Scenario: A source with no folders

- **WHEN** `sources.toml` names a source `build machine` with command `["my-overview-stream", "app"]` and no `folders`
- **THEN** that command runs one time, and its input is closed

### Requirement: The former key each_repository is refused

A `sources.toml` that names `each_repository` SHALL be refused. The plugin's log SHALL name the line, and SHALL say that `folders = true` replaces it. The default source SHALL run. (`never-break-an-existing-client@b539ef8e2413`)

#### Scenario: The former key

- **WHEN** `sources.toml` names a source with `each_repository = true`
- **THEN** the plugin's log names that line and says that `folders = true` replaces it
- **AND** the default source runs

## MODIFIED Requirements

### Requirement: Sources come from the user's configuration

The watcher SHALL run each source in `sources.toml` in the plugin's config directory: a name and a command that prints `sluice/overview@1` lines. With no `sources.toml`, the watcher SHALL run the one source `sluice overview --json --watch --stdin` with `folders = true`. The plugin SHALL NOT build a command of its own to reach another machine.

Before: the default source ran one time in each repository. Now: it runs one time for all of them. (`one-subscription-per-plugin@ba0cdba772ca`)

#### Scenario: No configuration

- **WHEN** panes are open in a repository with sluice's records and in one without, and there is no `sources.toml`
- **THEN** `sluice overview --json --watch --stdin` runs one time, and the popup holds only the repository with records

#### Scenario: Panes of two servers in one repository

- **WHEN** two herdr servers each have a pane in a repository with sluice's records, and there is no `sources.toml`
- **THEN** `sluice overview --json --watch --stdin` runs one time, and its input holds one `add` line for that repository

#### Scenario: The last pane leaves

- **WHEN** the last pane of any server in a repository closes, and the watcher next reads that server's panes
- **THEN** the watcher writes `remove` for the repository's folder on the source's input, and the popup and the dashboard no longer hold the repository

#### Scenario: A source of the user's

- **WHEN** `sources.toml` names a source `build machine` with command `["my-overview-stream", "app"]`
- **THEN** that command runs one time, and its documents are kept under their own `host` and `repository`

#### Scenario: Configuration it cannot read

- **WHEN** `sources.toml` holds a line outside the subset that the README states
- **THEN** the plugin's log names the line and the default source runs

### Requirement: The host runs one watcher and two processes for each source

The watcher SHALL run as one process beside its tick's sleep. Each source SHALL run as its command and one reader of its lines. The number of processes SHALL NOT change with the number of herdr servers or the number of repositories. (`one-subscription-per-plugin@ba0cdba772ca`)

#### Scenario: Three servers, two repositories

- **WHEN** three herdr servers have panes in two repositories with sluice's records, there is no `sources.toml`, and the source has run for a tick
- **THEN** the plugin's processes on the host are four: the watcher, its sleep, the source's command and its reader

#### Scenario: Ten repositories

- **WHEN** one herdr server has panes in ten repositories with sluice's records, there is no `sources.toml`, and the source has run for a tick
- **THEN** the plugin's processes on the host are four
