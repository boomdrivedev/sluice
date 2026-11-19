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
in
{
  # daemon-service, "Unset on nix-darwin" (record 0093 as amended
  # 2026-11-18; design D12 of `sluice-runs-as-a-daemon`).
  "The daemon is off unless turned on" =
    t.expect (!base.config.services.sluice.daemon.enable) "services.sluice.daemon.enable is true unset"
    ++ t.expect (
      !(base.config.launchd.user.agents ? sluice)
    ) "launchd.user.agents names sluice with the daemon unset";

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
