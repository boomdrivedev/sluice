# The nix-darwin module, darwinModules.default: services.sluice's file
# linked at ~/.config/sluice/config.toml for nix-darwin's primary user,
# the user who runs sluice, and sluice in the system profile (record 0098,
# core.51). sluice reads the file at its next command, so the module adds
# no agent, no daemon and nothing else for sluice to run (135, core.31),
# but the agent that keeps `sluice daemon` running where the host sets
# `services.sluice.daemon.enable` (137; record 0093 as amended 2026-11-18;
# design D9, D11 of `sluice-runs-as-a-daemon`): a launchd agent of the
# primary user's login, since nix-darwin installs user agents for that user
# alone, as Boomdrive's runner's agent is kept. Once the Nix store is
# mounted, it runs the module's sluice, or the one its PATH finds where the
# package is null, holding the job's own process by exec, so nix-darwin
# reloads it for a new package and for nothing else.
{ packages }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.sluice;
  file = pkgs.writeText "sluice-config.toml" cfg.text;

  user = config.system.primaryUser;
  home = if user == null then null else config.users.users.${user}.home or null;

  # The daemon's line: once the store's volume is mounted, the package's
  # sluice, or the PATH's where the package is null, by exec, so a stop's
  # signal reaches it (design D9, D11).
  program = if cfg.package == null then "sluice" else "${cfg.package}/bin/sluice";
  daemonLine = "/bin/wait4path /nix/store && exec ${lib.escapeShellArg program} daemon";

  # The profiles, then the operating system's folders: never a package's
  # own folder, so the daemon and a person's shell run one copy of each
  # tool.
  path = [
    "/etc/profiles/per-user/${user}/bin"
    "${home}/.nix-profile/bin"
    "${home}/.local/state/nix/profile/bin"
    "/run/current-system/sw/bin"
    "/nix/var/nix/profiles/default/bin"
    "/usr/local/bin"
    "/usr/bin"
    "/bin"
    "/usr/sbin"
    "/sbin"
  ];
in
{
  imports = [ (import ./options.nix { inherit packages; }) ];

  config = lib.mkIf cfg.enable {
    system.requiresPrimaryUser = [ "services.sluice.enable" ];

    assertions = [
      {
        assertion = user == null || home != null;
        message = "services.sluice links ~/.config/sluice/config.toml for nix-darwin's primary user ${toString user}, whose home users.users.${toString user}.home is not set: set it, as the home the file is linked in.";
      }
    ];

    environment.systemPackages = lib.optional (cfg.package != null) cfg.package;

    # Activation runs as root, with root's HOME, so the link is made as the
    # primary user under their own home. A link that fails says why and
    # stops nothing else of the system's activation.
    system.activationScripts.postActivation.text = ''
      sudo --user=${lib.escapeShellArg user} -- /usr/bin/env HOME=${lib.escapeShellArg home} /bin/sh ${./activate.sh} link ${file} || true
    '';

    launchd.user.agents.sluice = lib.mkIf cfg.daemon.enable {
      serviceConfig = {
        ProgramArguments = [
          "/bin/sh"
          "-c"
          daemonLine
        ];
        RunAtLoad = true;
        KeepAlive = true;
        # A daemon that exits is started again no more than twice a minute.
        ThrottleInterval = 30;
        ProcessType = "Interactive";
        # A stop ends the daemon alone: the runs it started go on (135).
        AbandonProcessGroup = true;
        # The time its tasks are given to finish their rounds, and a margin
        # (design D7).
        ExitTimeOut = 75;
        EnvironmentVariables = {
          HOME = home;
          PATH = lib.concatStringsSep ":" path;
        };
        # What it says before it holds the host and sends its output to
        # the host process's log.
        StandardOutPath = "${home}/Library/Logs/sluice-daemon.log";
        StandardErrorPath = "${home}/Library/Logs/sluice-daemon.log";
      };
    };
  };
}
