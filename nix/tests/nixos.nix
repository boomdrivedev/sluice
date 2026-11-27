# The host-settings-module spec on NixOS (record 0098): the module
# evaluated in a system of x86_64-linux whose normal users are alice, who
# runs sluice, and bob, its user activation's link, and the system
# profile, against the same system with the module not enabled; and the
# module imported twice.
t:
let
  inherit (t) lib;
  settings = import ./settings.nix;
  flakes = t.outputs.packages.x86_64-linux.default;

  base = t.nixosSystem { };
  every = t.nixosSystem { services.sluice.settings = settings.every; };
  own = t.nixosSystem (
    { pkgs, ... }:
    {
      services.sluice.package = pkgs.writeTextDir "bin/sluice" "";
    }
  );
  none = t.nixosSystem { services.sluice.package = null; };
  off = t.nixosSystem { services.sluice.enable = lib.mkForce false; };
  noUser = t.nixosSystem { services.sluice.user = lib.mkForce "carol"; };
  daemon = t.nixosSystem { services.sluice.daemon.enable = true; };
  daemonOwn = t.nixosSystem (
    { pkgs, ... }:
    {
      services.sluice = {
        package = pkgs.writeTextDir "bin/sluice" "";
        daemon.enable = true;
      };
    }
  );
  # Imported again under another module, as another flake's module that
  # imports it does.
  twice = t.nixosSystem { imports = [ { imports = [ t.outputs.nixosModules.sluice ]; } ]; };

  # Compared as text: a regular expression may not hold a store path.
  plain = builtins.unsafeDiscardStringContext;
  # The user activations as the system defines them, each by its name;
  # the option's value holds only the script made of them all.
  activationsOf =
    evaluated: lib.foldl' (a: b: a // b) { } evaluated.options.system.userActivationScripts.definitions;
  activationOf = evaluated: plain ((activationsOf evaluated).sluice.text or "");
  scriptOf = evaluated: plain evaluated.config.system.userActivationScripts.script;

  # The module's file holding `text`, byte for byte.
  fileOf = evaluated: text: (evaluated.pkgs.writeText "sluice-config.toml" text).outPath;
  # alice's link alone, with the file of `text`.
  linkOf =
    evaluated: text:
    plain ''if [ "$(id -un)" = alice ]; then ${evaluated.pkgs.runtimeShell} ${../modules/activate.sh} link ${fileOf evaluated text} || true; fi'';
  links =
    evaluated: line:
    t.expect (lib.hasInfix line (activationOf evaluated)) "NixOS's user activation sluice runs no `${line}`: ${activationOf evaluated}"
    ++ t.expect (lib.hasInfix line (scriptOf evaluated)) "NixOS's user activation script runs no `${line}`";

  profileOf = evaluated: map (p: p.outPath) evaluated.config.environment.systemPackages;
  installs =
    evaluated: name: package:
    t.expect (builtins.elem package.outPath (profileOf evaluated)) "NixOS's system profile holds no ${name}";
  installsNo =
    evaluated: name: package:
    t.expect (
      !builtins.elem package.outPath (profileOf evaluated)
    ) "NixOS's system profile holds ${name}";

  # The names under `path` of the system's configuration.
  namesOf = evaluated: path: builtins.attrNames (lib.attrByPath path { } evaluated.config);
  same =
    path:
    t.expect (namesOf base path == namesOf off path)
      "${lib.concatStringsSep "." path} names ${toString (namesOf base path)} with the module, and ${toString (namesOf off path)} without";
  lingerOf = evaluated: builtins.mapAttrs (_: u: u.linger) evaluated.config.users.users;
  activationNamesOf = evaluated: builtins.attrNames (activationsOf evaluated);
in
{
  # daemon-service, "Unset on NixOS" (record 0093 as amended 2026-11-18;
  # design D12 of `sluice-runs-as-a-daemon`).
  "The daemon is off unless turned on" =
    t.expect (!base.config.services.sluice.daemon.enable) "services.sluice.daemon.enable is true unset"
    ++ t.expect (
      !(base.config.systemd.user.services ? sluice)
    ) "systemd.user.services names sluice with the daemon unset"
    ++ t.expect (
      (lingerOf base).alice == (lingerOf off).alice
    ) "alice's linger changes with the module, the daemon unset";

  # daemon-service, "The daemon on" and "The daemon off" (design D1 of
  # `a-sluice-client-starts-no-host-under-a-service-manager`): with the
  # daemon on, the file says `[daemon] managed = true`, which the host may
  # set false; with it off, the file holds no `[daemon]`.
  "The daemon says it is managed" =
    let
      textOf = evaluated: evaluated.config.services.sluice.text;
      own = t.nixosSystem {
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

  # daemon-service, "The unit", and the user's activation keeping it (137;
  # record 0093 as amended 2026-11-18; design D10, D11 of
  # `sluice-runs-as-a-daemon`).
  "The daemon on NixOS" =
    let
      unit = daemon.config.systemd.user.services.sluice;
      program = "/run/current-system/sw/bin/sluice";
      keeps = "activate.sh keep alice ${program}";
    in
    t.expect (t.failures daemon == [ ]) "the system fails: ${toString (t.failures daemon)}"
    ++ t.expect (
      plain unit.serviceConfig.ExecStart == plain ''"${program}" "daemon"''
    ) "the unit runs ${plain unit.serviceConfig.ExecStart}, not ${plain program} daemon"
    ++ t.expect (
      unit.serviceConfig.Restart == "always"
    ) "the unit restarts ${unit.serviceConfig.Restart}"
    ++ t.expect (
      unit.serviceConfig.KillMode == "control-group"
    ) "the unit's stop kills ${unit.serviceConfig.KillMode}"
    ++ t.expect (
      unit.unitConfig.ConditionUser == "alice"
    ) "the unit runs for ${unit.unitConfig.ConditionUser}"
    ++ t.expect (
      unit.startLimitIntervalSec == 0
    ) "the unit is given up on after ${toString unit.startLimitIntervalSec}"
    ++ t.expect (
      unit.wantedBy == [ "default.target" ]
    ) "the unit is wanted by ${toString unit.wantedBy}"
    ++ t.expect daemon.config.users.users.alice.linger "alice does not linger"
    ++ t.expect (lib.hasInfix (plain keeps) (activationOf daemon)) "alice's activation runs no `${plain keeps}`: ${activationOf daemon}"
    ++ links daemon (lib.removeSuffix "; fi" (linkOf daemon (builtins.readFile ./files/managed.toml)));

  # daemon-service, "A new package" (D5 of `sluice-host-owns-its-workers`):
  # the unit names no package, so a new package changes no unit; the
  # activation has the daemon start afresh.
  "A new package changes no unit" =
    t.expect (t.failures daemonOwn == [ ]) "the system fails: ${toString (t.failures daemonOwn)}"
    ++ t.expect (
      daemonOwn.config.systemd.user.services.sluice.serviceConfig
      == daemon.config.systemd.user.services.sluice.serviceConfig
    ) "the unit changes with the package"
    ++ t.expect (
      activationOf daemonOwn == activationOf daemon
    ) "NixOS's user activation sluice changes with the package: ${activationOf daemonOwn}";

  "On NixOS" =
    t.expect (t.failures every == [ ]) "every key fails: ${toString (t.failures every)}"
    ++ links every (linkOf every (builtins.readFile ./files/every.toml));

  "A user the system does not declare" =
    t.expect (t.failures noUser != [ ]) "the module evaluates for a user users.users does not declare"
    ++
      lib.concatMap
        (
          words:
          t.expect (lib.hasInfix words (toString (t.failures noUser))) "the failure names no ${words}: ${toString (t.failures noUser)}"
        )
        [
          "services.sluice.user"
          "users.users.carol"
        ];

  "The flake's sluice" =
    t.expect (t.failures base == [ ]) "the system fails: ${toString (t.failures base)}"
    ++ installs base "the flake's sluice for x86_64-linux" flakes;

  "A host's own sluice" =
    installs own "the host's own sluice" own.config.services.sluice.package
    ++ installsNo own "the flake's sluice" flakes
    ++ t.expect (
      activationOf own == activationOf base
    ) "NixOS's user activation sluice changes with the package: ${activationOf own}";

  "No package" =
    installsNo none "the flake's sluice" flakes
    ++ t.expect (
      profileOf none == profileOf off
    ) "NixOS's system profile with no package is not the profile without the module"
    ++ links none (linkOf none (builtins.readFile ./files/empty.toml));

  "Nothing else on NixOS" =
    lib.concatMap same [
      [
        "environment"
        "etc"
      ]
      [
        "systemd"
        "services"
      ]
      [
        "systemd"
        "user"
        "services"
      ]
    ]
    ++ t.expect (lingerOf base == lingerOf off) "a user's linger changes with the module"
    ++
      t.expect (activationNamesOf base == lib.sort lib.lessThan (activationNamesOf off ++ [ "sluice" ]))
        "the user activations are ${toString (activationNamesOf base)} with the module, and ${toString (activationNamesOf off)} without";

  "The module imported twice on NixOS" =
    let
      evaluates = (builtins.tryEval twice.config.system.build.toplevel.drvPath).success;
      declared = builtins.length twice.options.services.sluice.enable.declarations;
      key = "github:agentplot/sluice#nixosModules.sluice";
      keyOf = module: module.key or null;
    in
    t.expect (
      keyOf t.outputs.nixosModules.sluice == key
    ) "nixosModules.sluice's key is ${toString (keyOf t.outputs.nixosModules.sluice)}, not ${key}"
    ++ t.expect (
      keyOf t.outputs.nixosModules.default == key
    ) "nixosModules.default's key is ${toString (keyOf t.outputs.nixosModules.default)}, not ${key}"
    ++ t.expect evaluates "a system importing the module twice does not evaluate"
    ++ lib.optionals evaluates (
      t.expect (declared == 1) "services.sluice.enable is declared ${toString declared} times"
      ++ links twice (linkOf twice (builtins.readFile ./files/empty.toml))
    );
}
