---
number: 98
title: sluice's flake gives a module that writes the host's own settings and nothing else
status: accepted
date: 2026-11-07
decision-makers:
- Chuck Swanberg
- the construct stage of sluice-publishes-a-module-for-its-host-settings
---

# sluice's flake gives a module that writes the host's own settings and nothing else

Status: accepted. That sluice's flake gives a module for its host settings is agentplot/boomdrive record 0023's decision, accepted by the user on 2026-11-06 ("Other than that nitpick I approve them all"), and principle 10 of agentplot/blueprints `design/principles.md`, from the user's talk of that day. This record is sluice's part of it. The choices within it below are the construct stage's own for the change `sluice-publishes-a-module-for-its-host-settings`: which options the module declares, how a key's path is held out of the store, how the file is rendered and placed, what the module installs, and how the gate checks it. Each is cheap to change until a release carries it.

Amended 2026-11-08: each module carries a key, `github:agentplot/sluice#<platform>Modules.sluice`, as agentplot/flume's flake keys its own, so a configuration that imports sluice's module itself and through agentplot/boomdrive's re-export (record 0023) declares `services.sluice` once, where unkeyed it declared every option twice and failed to evaluate. The key's form and its test are the code stage's own choice in the chore `sluices-module-is-keyed-as-flumes-is`, cheap to change later. The path not taken is the module as a plain path, which nix's module system takes once by its path: the module is given the flake's packages, so it is a function's value, which only a key lets nix take once.

Amended 2026-11-18 by record 0093's amendment of that day: the module declares the service that keeps `sluice daemon` running where the host sets `services.sluice.daemon.enable`. It is a launchd agent on nix-darwin and a systemd user unit on NixOS. Off by default, it adds nothing. "The package, and nothing else" below holds but for that service.

Deciders: the user, in the talk `research--the-flakes-configuration-holds-streams-and-repositories` of 2026-11-06, who asked "I want to make sure the flake also packages and configures crew and sluice" and settled that each tool's module is the tool's own flake's, re-exported by the Boomdrive flake. The construct stage for the shape below, from the family's modules as they stand.

Sources: agentplot/boomdrive record 0023 and its design `design/the-flakes-configuration/design.md`, with its sketch `deneb.nix`; agentplot/boomdrive record 0020 and its modules at 3f428cf (`nix/modules/options.nix`, `toml.nix`, `darwin.nix`, `nixos.nix`, `activate.sh`; `nix/check.sh`, `nix/inputs.nix`, `nix/tests/`); agentplot/flume record 0055 and its flake's `moduleOf`, which keys its modules; afterthought/crew's change `crew-publishes-a-flake-whose-module-writes-its-host-file`; agentplot/blueprints `design/principles.md` 10; clauses 35, 71, 74, 78, 87, 135 and 136; rules core.21, core.24, core.26 and core.31.

## Context and problem statement

sluice reads the host's own settings from `$XDG_CONFIG_HOME/sluice/config.toml`, `~/.config/sluice/config.toml` where that is unset (71; `crates/sluice/src/instance.rs`). It reads the file at every command and never writes it, and it refuses a field it does not know. Whoever keeps the host writes it. On a host configured by Nix, the fleet writes it today outside any module of sluice's.

Record 0023 has the Boomdrive flake configure a whole host, each tool by its own module. The tool's own flake publishes the module, so the tool can be used without Boomdrive. The Boomdrive flake re-exports it and sets its options. Each module writes its own tool's file, each key named as the file names it. No module writes another tool's file. sluice's flake builds the package alone.

Two things in sluice's file shape the module. Its `[forge] key` and `[signing] key` name private keys by their paths, and the Nix store is read by every user. And sluice is a plain command line with nothing installed for it to run (135).

## Considered options

1. **sluice's flake gives `services.sluice`, for nix-darwin and NixOS, in the shape of the family's modules.** Options are named as the file names them. The file is rendered at evaluation and linked at sluice's path for the user who runs sluice. A key is named by its path, outside the store. The module installs the package and nothing else.
2. A home-manager module. A nix-darwin or NixOS host need not run home-manager, and the family's modules are system modules.
3. The settings written by a closed set of options, each with its own type and bounds. Every bound of sluice's would then live twice, and a newer sluice's key would wait on the module.

## Decision outcome

Option 1.

- **The modules.** `darwinModules.sluice` and `nixosModules.sluice`, each also `default`, import one set of options given the flake's packages. Each is keyed `github:agentplot/sluice#<platform>Modules.sluice`, `darwin` or `nixos`, as Flume's are, so a configuration that imports it twice, itself and through another flake's module, declares `services.sluice` once. Their options are `services.sluice.enable`, `package` and `settings`, and `user` on NixOS. The flake's own files are `flake.nix` and `flake.lock` at the root, with everything else of it under `nix/`, as Boomdrive's are. The flake gives aarch64-darwin, aarch64-linux and x86_64-linux. Its nixpkgs has refused x86_64-darwin since 26.11, so a package of that system no longer evaluates, and no release builds one.
- **`settings` is the file, under its own names.** It is free-form TOML, as RFC 42 has it. The module declares each key sluice reads, with its TOML type and nothing finer: `runs` (`max`, `cores`, `reserved`, `systems`), `forge` (`app_id`, `key`, `discovery`), `signing` (`signer`, `key`), `peers` (`lease`, `alive`, `name`) and `caches` (`dir`, `socket`, `env`). A key left unset is left out, so sluice's own default holds. A key the module does not declare passes to the file, and sluice judges it as it judges every value. The module holds no default and no bound of sluice's.
- **A key is named by its path, outside the store.** `forge.key` and `signing.key` begin with `/` or `~/` and lie outside the store directory, or evaluation fails naming the option. A Nix path value fails by its type, so the module copies no key into the store. sluice refuses a key file others may read, a store file among them, but only once the key is already there for every user to read.
- **Rendered at evaluation.** The renderer is Boomdrive's `nix/modules/toml.nix` at 3f428cf, its header and messages naming sluice's module. The gate compares the text a module writes without building it, and one `settings` gives one text on either platform. nixpkgs has no renderer at evaluation. `pkgs.formats.toml` renders in a build, which the gate's offline check against no store cannot run. Importing Boomdrive's renderer would make sluice's flake an input of its own input.
- **Linked for the user who runs sluice.** The activation links `~/.config/sluice/config.toml` to the module's file. On nix-darwin this is for the primary user. On NixOS it is for `services.sluice.user`, which has no default. A link there is replaced. A file there that is no link is moved aside once, to `config.toml.before-nix`. Where that path is taken already, the activation changes neither, names both, and stops nothing else of the system's activation. This is Boomdrive's `activate.sh link`, for sluice's path. Only the file is linked, never the folder, so the folder still holds the host's keys, which the module never writes. sluice reads the file at its next command; the module restarts nothing.
- **The package, and nothing else.** `package` is the flake's sluice for the host's system, put in the system profile. A host may replace it, or set it to null to install none. The module adds no launchd agent or daemon, no systemd unit, no file under `/etc` and no list of repositories (135, core.31). It writes no file of the herdr plugin's, worktrunk's, Claude Code's, Flume's or the runner's (principle 10).
- **The managed layer alone.** The file the module writes is `config.toml`, which principle 10 calls the managed layer. sluice reads no local layer today. When it reads one, `config.local.toml` will be the operator's or a control plane's, never the module's.
- **sluice reads what the module writes.** One of sluice's own tests reads the module's rendered file, with every declared key set, through sluice's own reader, which refuses an unknown field. A declared key sluice does not know fails sluice's own tests.
- **The gate checks it.** The moon project `nix` checks the flake. Its tasks are `lint` (statix), `typecheck` and `test`, each evaluating offline and read-only against a dummy store, with inputs the devenv shell fetched, as Boomdrive's `nix/check.sh` does. Its format is `root:format`'s, by treefmt's nixfmt, deadnix, shellcheck and shfmt (core.21). The rendered files the cases compare are excluded from treefmt by name, since taplo would rewrite their bytes.

### Consequences

- Rule core.51 says what the module writes and what it never does; core.21 names the flake's `nix/` among the parts that are projects of their own.
- The configuration reference page, the README and the setting-up skill name the module, as a host's configuration under Nix.
- agentplot/boomdrive re-exports the module and sets its options, and drops sluice from its own `tools` once it does. That is boomdrive's change `the-flake-configures-each-tool-by-its-own-module`.
- afterthought/swancloud, once the module lands, passes its hosts' settings as `services.sluice.settings` and stops writing sluice's file.
- Until it does, a host that enables the module while another writer still links sluice's file has two writers of one path. Whichever activation runs last wins.
