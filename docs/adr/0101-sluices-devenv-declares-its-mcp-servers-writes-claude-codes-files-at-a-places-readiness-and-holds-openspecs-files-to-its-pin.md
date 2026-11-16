---
number: 101
title: sluice's devenv declares its MCP servers, writes Claude Code's files at a place's readiness, and holds OpenSpec's files to its pin
status: accepted
date: 2026-11-07
decision-makers:
- the code stage of the chore sluice-devenv-declares-its-mcp-servers
---

# sluice's devenv declares its MCP servers, writes Claude Code's files at a place's readiness, and holds OpenSpec's files to its pin

Status: accepted on 2026-11-07, the code stage's own choices under agentplot/boomdrive's record 0022, which the user accepted on 2026-11-07 and which asks each repository's change to confirm its places have their servers before their agents start. Each is cheap to change later. Amended 2026-11-11 on the user's word: the two files are copies devenv writes, not links into the store. Amended 2026-11-15 by record 0127: no task compares OpenSpec's files, and `write-openspec-claude.sh` writes them.

Sources: agentplot/boomdrive record 0022 and its amendment of 2026-11-07, at cac1d14 on boomdrive's `bolt/boomdrive-sessions-start-their-mcp-servers`; agentplot/flume's chore of the same name at f2d4483a; devenv 2.4's `src/modules/integrations/claude.nix` and `src/modules/files.nix` (`copyMode`); OpenSpec 1.14.3's `init` and `src/core/global-config.ts`; clause 147 and rules core.21, core.24 and core.46; deneb on 2026-11-11, where Claude Code sessions in worktrees of sluice and boomdrive each held about 16 watches on `/nix/store` itself, their reads of its 101,000 paths held the store's lock, and every nix program and the nix daemon hung until those sessions were stopped.

## Context and problem statement

Record 0022 has each repository of the family declare its MCP servers in `claude.code.mcpServers`, approve them by name in the project settings devenv writes, ignore `.mcp.json` and `.claude/settings.json` in git, move what its Claude Code settings hold into devenv's options, and track OpenSpec's Claude Code skills and commands as its pinned OpenSpec writes them. sluice tracked `.claude/settings.json` by hand, holding one hook, Vale on the prose a session writes, and OpenSpec's skills and commands as OpenSpec 1.14.1 wrote them while devenv pinned 1.14.3.

devenv writes the two files from a task of its own, `devenv:files`, which runs when a shell is entered. A sluice place's readiness is sluice's equip (147, core.46): it sources the script `devenv print-dev-env` prints, which holds the shell's `enterShell` and none of its tasks, so that task never runs there. devenv's Claude Code integration also adds a hook of its own wherever git-hooks are on, which sluice's are.

## Considered options

For a place's files:

1. **`enterShell` runs devenv's files script itself,** from the repository's root, writing nothing to standard output, so sourcing the printed script makes both files.
2. A toolchain that enters the shell and runs its tasks. It changes the environment every check runs under, and would run `proto install` inside the executor, where nothing may be installed.

For OpenSpec's files:

1. **A root task writes them afresh with the pinned OpenSpec, under a configuration of its own, and compares them whole;** `--write` replaces the tracked ones.
2. A check that each skill names the pinned release, as boomdrive's `root:toolchain` does. It passes a file edited by hand, and leaves writing them again to the user's own OpenSpec configuration, which must name the ten workflows.

## Decision outcome

Option 1 of each.

- **The servers.** devenv's hosted server, `mcp.devenv.sh`, alone. sluice needs no other: it builds on no hosted platform with a server of its own.
- **Their approval.** `enabledMcpjsonServers` holds the name of every server `claude.code.mcpServers` names, so one added there is approved with it. devenv has no option for the key, so it is added to the settings file devenv writes.
- **A place's files.** `enterShell` runs `devenv:files`'s script. A place has both files once it is ready, before its agent starts, and a shell entered later finds them made. Sourced inside the executor at each check run, the script writes only in the place: devenv's own state under `.devenv/state`, and the two files. Each is a regular file devenv writes afresh at every shell entry and readiness, replacing what is at its path by a rename (`copyMode = "copy"`), never a link into the store (amended 2026-11-11): Claude Code watches a project's settings by the file's real path, so a link had each session watching the store's own directory and reading all of it again whenever a build added or removed a path. A working copy where they cannot be written says so on standard error and still gets its shell.
- **What the settings held.** The Vale hook is `claude.code.hooks.vale`, as it was. devenv's own `git-hooks-run` is off: it would run prek after every edit, treefmt and, once a Rust file is staged, clippy over the workspace, in each turn; the pre-commit hook and the gate run them where they ran before. treefmt's exclude for the settings file goes, since git ignores it.
- **OpenSpec's files.** `write-openspec-claude.sh` refuses an `openspec` other than the release devenv.nix pins (`openspecVersion`, core.24), has it write its Claude Code files into a scratch directory with the ten workflows sluice tracks, propose, explore, new, continue, apply, ff, sync, archive, bulk-archive and verify, as skills and as commands, and replaces the tree's with them. No task compares them, since no task reads a Markdown file (amended 2026-11-15, record 0127; principle 16; core.21). Moving the pin is followed by `sh write-openspec-claude.sh` in the shell.

### Consequences

- A worktree that checks out this revision loses the tracked settings file, and gets devenv's copy at its next shell or readiness, written over whatever is at either path, a link into the store left by an earlier revision among them (amended 2026-11-11). A session started before then keeps its watches on the store until it is restarted.
- The tracked OpenSpec files are 1.14.3's.
- boomdrive and flume hold their OpenSpec files to their pin by the release each skill names. That is one thing done two ways in the family, told to the orchestrator for the family to settle on one.
