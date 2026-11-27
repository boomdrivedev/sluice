# The host-settings-module spec on nix-darwin (record 0098): the module
# evaluated in a system of aarch64-darwin whose primary user is alice, at
# /Users/alice, its activation's link, and the system profile, against
# the same system with the module not enabled; and the module imported
# twice.
t:
let
  inherit (t) lib;
  settings = import ./settings.nix;
  flakes = t.outputs.packages.aarch64-darwin.default;

  base = t.darwinSystem { };
  every = t.darwinSystem { services.sluice.settings = settings.every; };
  own = t.darwinSystem (
    { pkgs, ... }:
    {
      services.sluice.package = pkgs.writeTextDir "bin/sluice" "";
    }
  );
  none = t.darwinSystem { services.sluice.package = null; };
  off = t.darwinSystem { services.sluice.enable = lib.mkForce false; };
  noHome = t.darwinSystem { users.users.alice.home = lib.mkForce null; };
  daemon = t.darwinSystem { services.sluice.daemon.enable = true; };
  daemonOwn = t.darwinSystem (
    { pkgs, ... }:
    {
      services.sluice = {
        package = pkgs.writeTextDir "bin/sluice" "";
        daemon.enable = true;
      };
    }
  );
  daemonNone = t.darwinSystem {
    services.sluice = {
      package = null;
      daemon.enable = true;
    };
  };
  agentOf = evaluated: evaluated.config.launchd.user.agents.sluice.serviceConfig;
  # Imported again under another module, as another flake's module that
  # imports it does.
  twice = t.darwinSystem { imports = [ { imports = [ t.outputs.darwinModules.sluice ]; } ]; };

  # Compared as text: a regular expression may not hold a store path.
  plain = builtins.unsafeDiscardStringContext;
  activationOf = evaluated: plain evaluated.config.system.activationScripts.postActivation.text;
  scriptOf = evaluated: plain evaluated.config.system.activationScripts.script.text;

  # The module's file holding `text`, byte for byte.
  fileOf = evaluated: text: (evaluated.pkgs.writeText "sluice-config.toml" text).outPath;
  # The link's line, as alice under her home, with the file of `text`.
  linkOf =
    evaluated: text:
    plain "sudo --user=alice -- /usr/bin/env HOME=/Users/alice /bin/sh ${../modules/activate.sh} link ${fileOf evaluated text} || true";
  emptyLink = linkOf base (builtins.readFile ./files/empty.toml);
  links =
    evaluated: line:
    t.expect (lib.hasInfix line (activationOf evaluated)) "nix-darwin's activation runs no `${line}`: ${activationOf evaluated}";

  profileOf = evaluated: map (p: p.outPath) evaluated.config.environment.systemPackages;
  installs =
    evaluated: name: package:
    t.expect (builtins.elem package.outPath (profileOf evaluated)) "nix-darwin's system profile holds no ${name}";
  installsNo =
    evaluated: name: package:
    t.expect (
      !builtins.elem package.outPath (profileOf evaluated)
    ) "nix-darwin's system profile holds ${name}";

  # The names under `path` of the system's configuration.
  namesOf = evaluated: path: builtins.attrNames (lib.attrByPath path { } evaluated.config);
  same =
    path:
    t.expect (namesOf base path == namesOf off path)
      "${lib.concatStringsSep "." path} names ${toString (namesOf base path)} with the module, and ${toString (namesOf off path)} without";
  # The names under `path` with the daemon on, against the module alone.
  sameWithDaemon =
    path:
    t.expect (namesOf daemon path == namesOf base path)
      "${lib.concatStringsSep "." path} names ${toString (namesOf daemon path)} with the daemon, and ${toString (namesOf base path)} without";
in
{
  # daemon-service, "Unset on nix-darwin" (record 0093 as amended
  # 2026-11-18; design D12 of `sluice-runs-as-a-daemon`).
  "The daemon is off unless turned on" =
    t.expect (!base.config.services.sluice.daemon.enable) "services.sluice.daemon.enable is true unset"
    ++ t.expect (
      !(base.config.launchd.user.agents ? sluice)
    ) "launchd.user.agents names sluice with the daemon unset";

  # daemon-service, "The daemon on" and "The daemon off" (design D1 of
  # `a-sluice-client-starts-no-host-under-a-service-manager`): with the
  # daemon on, the file says `[daemon] managed = true`, which the host may
  # set false; with it off, the file holds no `[daemon]`.
  "The daemon says it is managed" =
    let
      textOf = evaluated: evaluated.config.services.sluice.text;
      own = t.darwinSystem {
        services.sluice = {
          daemon.enable = true;
          settings.daemon.managed = false;
        };
      };
    in
    t.expect (
      textOf daemon == builtins.readFile ./files/managed.toml
    ) "the file with the daemon on is not nix/tests/files/managed.toml: ${textOf daemon}"
    ++ t.expect (
      !lib.hasInfix "[daemon]" (textOf base)
    ) "the file with the daemon off holds [daemon]: ${textOf base}"
    ++ t.expect (lib.hasInfix "[daemon]\nmanaged = false\n" (textOf own)) "the host's own managed = false is not kept: ${textOf own}";

  # daemon-service, "The agent" (137; record 0093 as amended 2026-11-18;
  # design D9, D11 of `sluice-runs-as-a-daemon`).
  "The daemon on nix-darwin" =
    let
      agent = agentOf daemon;
      line = "/bin/wait4path /nix/store && exec /run/current-system/sw/bin/sluice daemon";
      path = agent.EnvironmentVariables.PATH or "";
      handOver = "/run/current-system/sw/bin/sluice hand-over || true";
    in
    t.expect (t.failures daemon == [ ]) "the system fails: ${toString (t.failures daemon)}"
    ++ t.expect (
      map plain agent.ProgramArguments == [
        "/bin/sh"
        "-c"
        (plain line)
      ]
    ) "the agent runs ${toString (map plain agent.ProgramArguments)}, not /bin/sh -c ${plain line}"
    ++ t.expect (agent.RunAtLoad == true) "the agent does not run at load"
    ++ t.expect (agent.KeepAlive == true) "the agent is not kept alive"
    ++ t.expect (agent.AbandonProcessGroup == false) "a stop does not end the agent's process group"
    ++ t.expect (lib.hasInfix handOver (activationOf daemon)) "the activation runs no `${handOver}`: ${activationOf daemon}"
    ++ t.expect (
      !lib.hasInfix "hand-over" (activationOf base)
    ) "the activation asks a hand-over with the daemon off: ${activationOf base}"
    ++ t.expect (
      agent.ThrottleInterval == 30
    ) "the agent's throttle is ${toString agent.ThrottleInterval}, not 30"
    ++ t.expect (
      agent.ExitTimeOut == 75
    ) "the agent's exit time out is ${toString agent.ExitTimeOut}, not 75"
    ++ t.expect (
      agent.EnvironmentVariables.HOME == "/Users/alice"
    ) "the agent's HOME is ${toString agent.EnvironmentVariables.HOME}"
    ++ t.expect (lib.hasPrefix "/etc/profiles/per-user/alice/bin:" path) "the agent's PATH is ${path}"
    ++ t.expect (
      agent.StandardErrorPath == "/Users/alice/Library/Logs/sluice-daemon.log"
    ) "the agent's errors go to ${toString agent.StandardErrorPath}";

  # daemon-service, "A new package" (D5 of `sluice-host-owns-its-workers`):
  # the agent names no package, so a new package changes no agent and
  # nix-darwin reloads none; the activation has the daemon start afresh.
  "A new package changes no agent" =
    t.expect (t.failures daemonOwn == [ ]) "the system fails: ${toString (t.failures daemonOwn)}"
    ++ t.expect (agentOf daemonOwn == agentOf daemon) "the agent changes with the package"
    ++ t.expect (
      activationOf daemonOwn == activationOf daemon
    ) "nix-darwin's activation changes with the package: ${activationOf daemonOwn}";

  # daemon-service, "No package" (design D11 of `sluice-runs-as-a-daemon`).
  "The daemon with no package" =
    let
      line = "/bin/wait4path /nix/store && exec sluice daemon";
    in
    t.expect (t.failures daemonNone == [ ]) "the system fails: ${toString (t.failures daemonNone)}"
    ++ t.expect (
      lib.last (agentOf daemonNone).ProgramArguments == line
    ) "the agent's line is ${lib.last (agentOf daemonNone).ProgramArguments}, not ${line}";

  # host-settings-module, "Only the daemon's service with the daemon on"
  # (design D12 of `sluice-runs-as-a-daemon`).
  "Only the daemon's service" =
    t.expect
      (
        namesOf daemon [
          "launchd"
          "user"
          "agents"
        ] == lib.sort lib.lessThan (
          namesOf base [
            "launchd"
            "user"
            "agents"
          ]
          ++ [ "sluice" ]
        )
      )
      "launchd.user.agents names ${
        toString (
          namesOf daemon [
            "launchd"
            "user"
            "agents"
          ]
        )
      } with the daemon, and ${
        toString (
          namesOf base [
            "launchd"
            "user"
            "agents"
          ]
        )
      } without"
    ++ lib.concatMap sameWithDaemon [
      [
        "environment"
        "etc"
      ]
      [
        "launchd"
        "agents"
      ]
      [
        "launchd"
        "daemons"
      ]
      [
        "users"
        "users"
      ]
    ];

  "On nix-darwin" =
    t.expect (t.failures every == [ ]) "every key fails: ${toString (t.failures every)}"
    ++ links every (linkOf every (builtins.readFile ./files/every.toml));

  "A primary user with no home" =
    t.expect (t.failures noHome != [ ]) "the module evaluates for a primary user with no home"
    ++ t.expect (lib.hasInfix "users.users.alice.home" (toString (t.failures noHome))) "the failure names no users.users.alice.home: ${toString (t.failures noHome)}";

  "The flake's sluice" =
    t.expect (t.failures base == [ ]) "the system fails: ${toString (t.failures base)}"
    ++ installs base "the flake's sluice for aarch64-darwin" flakes;

  "A host's own sluice" =
    installs own "the host's own sluice" own.config.services.sluice.package
    ++ installsNo own "the flake's sluice" flakes
    ++ t.expect (
      activationOf own == activationOf base
    ) "nix-darwin's activation changes with the package: ${activationOf own}";

  "No package" =
    installsNo none "the flake's sluice" flakes
    ++ t.expect (
      profileOf none == profileOf off
    ) "nix-darwin's system profile with no package is not the profile without the module"
    ++ links none emptyLink;

  # The names of what a system adds, with the module and the flake's
  # package and without the module; and the whole activation with no
  # package, the link's line taken out, without the module, since the
  # package changes the profile's own steps (`applications`).
  "Nothing else on nix-darwin" =
    lib.concatMap same [
      [
        "environment"
        "etc"
      ]
      [
        "launchd"
        "agents"
      ]
      [
        "launchd"
        "daemons"
      ]
      [
        "launchd"
        "user"
        "agents"
      ]
      [
        "users"
        "users"
      ]
    ]
    ++ t.expect (lib.hasInfix emptyLink (scriptOf none)) "nix-darwin's activation runs no `${emptyLink}`"
    ++ t.expect (
      builtins.replaceStrings [ "${emptyLink}\n" ] [ "" ] (scriptOf none) == scriptOf off
    ) "nix-darwin's activation, the link's line taken out, is not the activation without the module";

  "The module imported twice on nix-darwin" =
    let
      evaluates = (builtins.tryEval twice.system.drvPath).success;
      declared = builtins.length twice.options.services.sluice.enable.declarations;
      key = "github:agentplot/sluice#darwinModules.sluice";
      keyOf = module: module.key or null;
    in
    t.expect (
      keyOf t.outputs.darwinModules.sluice == key
    ) "darwinModules.sluice's key is ${toString (keyOf t.outputs.darwinModules.sluice)}, not ${key}"
    ++ t.expect (
      keyOf t.outputs.darwinModules.default == key
    ) "darwinModules.default's key is ${toString (keyOf t.outputs.darwinModules.default)}, not ${key}"
    ++ t.expect evaluates "a system importing the module twice does not evaluate"
    ++ lib.optionals evaluates (
      t.expect (declared == 1) "services.sluice.enable is declared ${toString declared} times"
      ++ links twice emptyLink
    );
}
