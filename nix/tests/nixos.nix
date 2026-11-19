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
