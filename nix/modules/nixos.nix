# The NixOS module, nixosModules.default: services.sluice's file linked at
# ~/.config/sluice/config.toml for services.sluice.user, the user who runs
# sluice, in that user's own activation, which runs at their manager's
# start and at a switch, and sluice in the system profile (record 0098,
# core.51). sluice reads the file at its next command, so the module adds
# no unit, no lingering and nothing else for sluice to run (135,
# core.31), but the unit that keeps `sluice daemon` running where the host
# sets `services.sluice.daemon.enable` (137; record 0093 as amended
# 2026-11-18; design D10, D11 of `sluice-runs-as-a-daemon`): a systemd
# user unit of services.sluice.user, who lingers, so it runs from boot
# with no one logged in, as Boomdrive's runner's unit is kept. The unit
# runs the system profile's sluice, so a new package changes no unit. The
# user's activation, after the link, starts the unit where it is not
# running, and otherwise has the running daemon start afresh as the
# profile's sluice where it runs another, which keeps its runs (D5 of
# `sluice-host-owns-its-workers`).
{ packages }:
{
  config,
  lib,
  pkgs,
  utils,
  ...
}:
let
  cfg = config.services.sluice;
  file = pkgs.writeText "sluice-config.toml" cfg.text;

  inherit (cfg) user;
  account =
    config.users.users.${user} or {
      isNormalUser = false;
      isSystemUser = false;
    };
  home = account.home or "/home/${user}";

  # The sluice the daemon runs: the system profile's, where the module
  # installs its package, or the PATH's where the package is null.
  program = if cfg.package == null then "sluice" else "/run/current-system/sw/bin/sluice";
  # The daemon's command line: the program, through the runtime shell where
  # it is the PATH's, since systemd looks a bare name up in its own folders
  # and not the unit's PATH (design D11).
  command =
    if cfg.package == null then
      [
        pkgs.runtimeShell
        "-c"
        "exec sluice daemon"
      ]
    else
      [
        program
        "daemon"
      ];

  # The profiles: never a package's own folder, so the daemon and a
  # person's shell run one copy of each tool.
  path = [
    "/etc/profiles/per-user/${user}/bin"
    "${home}/.nix-profile/bin"
    "${home}/.local/state/nix/profile/bin"
    "/run/wrappers/bin"
    "/run/current-system/sw/bin"
    "/nix/var/nix/profiles/default/bin"
  ];

  # What `keep` runs, which the user's activation does not have.
  keepPath = lib.makeBinPath [ config.systemd.package ];
  keeping = lib.optionalString cfg.daemon.enable "; PATH=${keepPath}:${lib.concatStringsSep ":" path}:$PATH ${pkgs.runtimeShell} ${./activate.sh} keep ${
    lib.escapeShellArgs [
      user
      program
    ]
  }";
in
{
  imports = [ (import ./options.nix { inherit packages; }) ];

  options.services.sluice.user = lib.mkOption {
    type = lib.types.str;
    example = "alice";
    description = "The user who runs sluice, whose `~/.config/sluice/config.toml` is linked to the module's file in their own activation, and whose user manager runs `sluice daemon` where `services.sluice.daemon.enable` is set.";
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = account.isNormalUser || account.isSystemUser;
        message = "services.sluice.user is ${user}, whom users.users.${user} does not declare: declare the user who runs sluice, in whose home the file is linked.";
      }
    ];

    environment.systemPackages = lib.optional (cfg.package != null) cfg.package;

    # Run by every user's activation, inside which a job is never waited
    # for; the user who runs sluice alone links, and keeps the daemon's
    # unit where the daemon is on. A link that fails says why and stops
    # nothing else of the activation.
    system.userActivationScripts.sluice.text = ''
      if [ "$(id -un)" = ${lib.escapeShellArg user} ]; then ${pkgs.runtimeShell} ${./activate.sh} link ${file} || true${keeping}; fi
    '';

    users.users.${user}.linger = lib.mkIf cfg.daemon.enable true;

    systemd.user.services.sluice = lib.mkIf cfg.daemon.enable {
      description = "sluice daemon, sluice's host process kept running";
      wantedBy = [ "default.target" ];
      unitConfig.ConditionUser = user;
      # The profiles' PATH alone; the user's manager gives HOME and
      # XDG_RUNTIME_DIR.
      enableDefaultPath = false;
      environment.PATH = lib.concatStringsSep ":" path;
      # Its command line names no package; the user's activation has the
      # daemon start afresh as a new sluice.
      restartIfChanged = false;
      serviceConfig = {
        ExecStart = utils.escapeSystemdExecArgs command;
        # A daemon that exits is started again no more than twice a
        # minute, and never given up on.
        Restart = "always";
        RestartSec = "30s";
        # A stop ends every process of the unit: the daemon ends its runs as
        # it stops, and systemd ends what is left
        # (`sluices-runs-end-with-its-host-process@e814b716f3b1`).
        KillMode = "control-group";
        # The time its tasks are given to finish their rounds, and a
        # margin (design D7).
        TimeoutStopSec = "75s";
      };
      startLimitIntervalSec = 0;
    };
  };
}
