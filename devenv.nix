{
  pkgs,
  lib,
  config,
  ...
}:

let
  # A tool's own release builds: nixpkgs trails proto and worktrunk by
  # minor versions or more. Linux takes the static musl build, so no loader
  # needs patching.
  fromRelease =
    {
      name,
      repo,
      version,
      bins,
      hashes,
    }:
    let
      system = pkgs.stdenv.hostPlatform.system;
      target =
        {
          aarch64-darwin = "aarch64-apple-darwin";
          x86_64-darwin = "x86_64-apple-darwin";
          aarch64-linux = "aarch64-unknown-linux-musl";
          x86_64-linux = "x86_64-unknown-linux-musl";
        }
        .${system} or (throw "${name} ${version}: no release build for ${system}");
    in
    pkgs.stdenvNoCC.mkDerivation {
      pname = name;
      inherit version;
      src = pkgs.fetchurl {
        url = "https://github.com/${repo}/releases/download/v${version}/${name}-${target}.tar.xz";
        sha256 = hashes.${target};
      };
      sourceRoot = "${name}-${target}";
      installPhase = ''
        runHook preInstall
        install -Dm755 -t $out/bin ${pkgs.lib.concatStringsSep " " bins}
        runHook postInstall
      '';
    };

  proto = fromRelease {
    name = "proto_cli";
    repo = "moonrepo/proto";
    version = "0.63.1";
    bins = [
      "proto"
      "proto-shim"
    ];
    hashes = {
      aarch64-apple-darwin = "8bfde4c862381eeb6e82d0858bb6215732cbd1e384e4eae45da8fa08095e0ffd";
      x86_64-apple-darwin = "aaaff78c7125de1b765f00177f35da75ee16fb1363e07632ace27213ff5d893b";
      aarch64-unknown-linux-musl = "111f5b6a38d3979ebafbc095c8d91ef69c99d1f018addb3582179d4ab91b3441";
      x86_64-unknown-linux-musl = "8596b98ccd833d1203e71931ba25e866d3239f9d3859c922a035a7b830f29d17";
    };
  };

  # worktrunk, which the tools tier and e2e drive: the version they are
  # tested against, here and in CI.
  worktrunk = fromRelease {
    name = "worktrunk";
    repo = "max-sixty/worktrunk";
    version = "0.82.0";
    bins = [
      "wt"
      "git-wt"
    ];
    hashes = {
      aarch64-apple-darwin = "6945bc82e0513d0b974c99fb1d30de24e448697fd533ed6b3f442c66a1b3620c";
      x86_64-apple-darwin = "d9c87668baa1aad2b44d5bfb08074609c5e4a186a0948dfed6e65d6e6c5a7744";
      aarch64-unknown-linux-musl = "b907f891bc051dcfc13fa479194e156b356f434a37103522ea1fe1b7ded899b5";
      x86_64-unknown-linux-musl = "d1e38cf36d8c14ef0682a6d6617f8a512b1a971feb3dfa437419d00136e19a3e";
    };
  };

  # The application toolchain .prototools pins, on PATH: what the shell
  # activates, and what a git hook, which runs outside any shell a session
  # opened, activates for itself. proto's home is this repository's, in the
  # git common dir so every worktree shares it: moon puts $PROTO_HOME/bin on
  # every task's PATH, and the user's own proto installs (an older moon
  # among them) must never shadow the system tools above. Only this
  # repository's pins are read (no global .prototools).
  protoHome = ''
    PROTO_HOME="$(${lib.getExe pkgs.git} rev-parse --path-format=absolute --git-common-dir)/proto"
    export PROTO_HOME
  '';
  activate = ''
    ${protoHome}
    eval "$(${proto}/bin/proto activate bash --export --no-bin --config-mode upwards 2>/dev/null)"
  '';

  # The lint gate.
  lint = pkgs.writeShellApplication {
    name = "lint";
    runtimeInputs = [ pkgs.git ];
    text = ''
      cd "$(git rev-parse --show-toplevel)"
      ${activate}
      cargo clippy --workspace --all-targets -- -D warnings
    '';
  };

  # treefmt's rustfmt: the pinned toolchain's.
  rustfmt = pkgs.writeShellApplication {
    name = "rustfmt";
    text = ''
      ${activate}
      exec rustfmt "$@"
    '';
  };

  # target/ grows unseen across worktrees until the disk is full.
  disk = pkgs.writeShellApplication {
    name = "disk";
    runtimeInputs = [
      pkgs.git
      pkgs.coreutils
      pkgs.gawk
    ];
    text = ''
      root="$(git rev-parse --show-toplevel)"
      limit=''${SLUICE_TARGET_LIMIT_GB:-25}
      target=0
      [ -d "$root/target" ] && target=$(($(du -sk "$root/target" | cut -f1) / 1048576))
      used="$(df -P "$root" | awk 'NR == 2 { print $5 }' | tr -d '%')"
      echo "target/ ''${target} GB, the disk ''${used} per cent full"
      status=0
      if [ "$target" -gt "$limit" ]; then
        echo "target/ is ''${target} GB, over the ''${limit} GB bar: run 'cargo clean' here and in the worktrees beside this one" >&2
        status=1
      fi
      if [ "$used" -ge 90 ]; then
        echo "the disk is ''${used} per cent full: a build on a full disk is slow and its processes are killed" >&2
        status=1
      fi
      exit $status
    '';
  };

  # Vale checks the documentation site's prose (.vale.ini, styles/Sluice).
  # From its own release: nixpkgs trails it. Bump the version and the four
  # hashes from the release's checksums file.
  valeVersion = "3.24.0";
  valeAssets = {
    aarch64-darwin = {
      os = "macOS_arm64";
      sha256 = "87b513f26499f6657c15cf7175ba5b058a7c1ff2bb2d4e07265db2dd43451e44";
    };
    x86_64-darwin = {
      os = "macOS_64-bit";
      sha256 = "eeb39e86f1daac27cc2a77fef3af1824d50559bdc22329fb25fb921c80c79472";
    };
    aarch64-linux = {
      os = "Linux_arm64";
      sha256 = "104cbd349279e50c4cb40adb70adf345d93cc4a3a0191f9e68bdccb97086b582";
    };
    x86_64-linux = {
      os = "Linux_64-bit";
      sha256 = "867534ddb678abca7f214bc4706ead0922bd5252ffe8035fc227013f66c8b214";
    };
  };
  valeAsset = valeAssets.${pkgs.stdenv.hostPlatform.system};
  vale = pkgs.stdenvNoCC.mkDerivation {
    pname = "vale";
    version = valeVersion;
    src = pkgs.fetchurl {
      url = "https://github.com/errata-ai/vale/releases/download/v${valeVersion}/vale_${valeVersion}_${valeAsset.os}.tar.gz";
      inherit (valeAsset) sha256;
    };
    sourceRoot = ".";
    installPhase = "install -Dm755 vale $out/bin/vale";
  };

  # Claude Code, run by the plugins' projects and by root:marketplace
  # (plugins/claude.sh): the release the board's engine-types were written
  # by, so every host checks the plugins against one engine, and never the
  # user's own `claude`, which may be a wrapper adding options `claude
  # plugin test` refuses. Bump the version and the four checksums from the
  # release's manifest.zst.json, and copy the board's engine-types again
  # from a session of it (plugins/sluice-board/README.md).
  claudeVersion = "2.1.296";
  claudeAssets = {
    aarch64-darwin = {
      platform = "darwin-arm64";
      sha256 = "26426aa3dcf41b0f554951766c3c583cf243345ca3d32fa590caf84685be5f97";
    };
    x86_64-darwin = {
      platform = "darwin-x64";
      sha256 = "6f1d6f926a303c423eb4815a01764165ed4375054bfa8ef6203e55fc806f50ac";
    };
    aarch64-linux = {
      platform = "linux-arm64";
      sha256 = "23aa91437a06eff80a65bfc32b0e4b5a6dda3e821019c543696c154274e56913";
    };
    x86_64-linux = {
      platform = "linux-x64";
      sha256 = "fac1d78be9446052ac084878c04c2648abb6b22d53279a685fa19ee517d74410";
    };
  };
  claudeAsset = claudeAssets.${pkgs.stdenv.hostPlatform.system};
  claude = pkgs.stdenvNoCC.mkDerivation {
    pname = "claude-code";
    version = claudeVersion;
    src = pkgs.fetchurl {
      url = "https://downloads.claude.ai/claude-code-releases/${claudeVersion}/${claudeAsset.platform}/claude.zst";
      inherit (claudeAsset) sha256;
    };
    dontUnpack = true;
    # The binary carries its program after the runtime; stripping drops it.
    dontStrip = true;
    nativeBuildInputs = [
      pkgs.zstd
      pkgs.makeBinaryWrapper
    ]
    ++ lib.optionals pkgs.stdenv.hostPlatform.isElf [ pkgs.autoPatchelfHook ];
    installPhase = ''
      mkdir -p $out/bin
      unzstd -q $src -o $out/bin/.claude
      chmod 755 $out/bin/.claude
      makeWrapper $out/bin/.claude $out/bin/claude \
        --set DISABLE_AUTOUPDATER 1 \
        --set DISABLE_INSTALLATION_CHECKS 1
    '';
  };

  # The flake's checks' tools (record 0098): the nix nix:typecheck and
  # nix:test evaluate with, and the statix nix:lint runs. nixpkgs' builds,
  # pinned by devenv.lock and asserted to be these versions, so every host
  # that runs the gate runs the same (core.24). treefmt.nix's nixfmt and
  # deadnix are from the same nixpkgs.
  nixVersion = "2.34.8";
  statixVersion = "0.5.8-unstable-2026-07-17";

  # The Rust crates' test tasks run under cargo-nextest, which runs each
  # test in a process of its own and fails a test that leaves a process
  # holding its output (the change `sluice-host-owns-its-workers`, design
  # D9): nixpkgs' build, pinned by devenv.lock and asserted to be this
  # version, so every host that runs the gate runs the same (core.24).
  nextestVersion = "0.9.143";

  # The linter of the CloudFormation template `sluice init codebuild`
  # writes, which treefmt.nix runs over its source as actionlint runs over
  # the workflows' (record 0119; core.21): nixpkgs' build, pinned by
  # devenv.lock and asserted to be this version, so every host that runs
  # the gate lints it alike (core.24).
  cfnLintVersion = "1.43.3";
  cfnLint =
    assert pkgs.python3Packages.cfn-lint.version == cfnLintVersion;
    pkgs.python3Packages.cfn-lint;

  # OpenSpec, which root:specs validates openspec/specs with: the release
  # the specs are written with, built as nixpkgs builds its older one. Bump
  # the version, then each hash from the mismatch the build reports, then
  # write OpenSpec's Claude Code skills and commands under .claude/ again
  # with it, `sh write-openspec-claude.sh` in the shell, since the tree
  # tracks them as this release writes them (records 0101, 0127).
  openspecVersion = "1.14.3";
  openspec = pkgs.openspec.overrideAttrs (
    finalAttrs: _: {
      version = openspecVersion;
      src = pkgs.fetchFromGitHub {
        owner = "Fission-AI";
        repo = "OpenSpec";
        tag = "v${finalAttrs.version}";
        hash = "sha256-tdTX0nkDe/aNZgLckh6/B6BtoSb40w0Y5qzg+d1LjHU=";
      };
      pnpmDeps = pkgs.fetchPnpmDeps {
        inherit (finalAttrs) pname version src;
        pnpm = pkgs.pnpm_10;
        fetcherVersion = 3;
        hash = "sha256-fKm4sBdTDeQfIiE0k301jtRbiuJrBE7YIEBu8RZxuXI=";
      };
    }
  );

  # devenv's own script that makes the files it declares, Claude Code's
  # among them, run from the repository's root.
  writeFiles = ''
    cd ${lib.escapeShellArg config.devenv.root} || exit
    ${config.tasks."devenv:files".exec}
  '';

in
{
  # The system tools are this file's, pinned by devenv.lock; the application
  # tools (moon, Rust) are .prototools', installed by the
  # sluice:proto:install task on entering the shell and activated by
  # enterShell. Every cargo and moon command in this
  # repository runs inside `devenv shell`.
  packages = [
    pkgs.git
    pkgs.jq
    # Debian's /bin/sh, which the herdr plugin's tests also run under.
    pkgs.dash
    # GNU awk, under which the herdr plugin's tests decode links in a UTF-8
    # locale as Linux users' awk would.
    pkgs.gawk
    pkgs.git-cliff
    proto
    worktrunk
    vale
    claude
    openspec
    # The board's type check (plugin-board:typecheck); its engine-types
    # ask for 5.4 or later.
    pkgs.typescript
    pkgs.bun
    # The site's build (site/package.json): its scripts and Astro run under
    # Node.js, the major the Pages runner's image carries, rather than bun
    # standing in for a node the shell lacks.
    pkgs.nodejs_22
    # The container engine's client, which the stack acceptance test
    # (ADR 0016) and its release command run as `docker`: the shell's own,
    # so a run finds it on every host (core.24), never only where the
    # host's PATH holds one, as a session's may and the queue's need not.
    # It reaches the engine as the user's client does, by DOCKER_HOST or
    # the user's docker context.
    pkgs.docker-client
    # The stack acceptance test's browser test (ADR 0016): Playwright and
    # the one browser it drives, from Nix, read-only for the sandbox.
    pkgs.playwright-test
    # The plugins' screenshots, optimized losslessly as they are taken
    # (plugins/screenshots-capture.sh).
    pkgs.oxipng
    (
      assert pkgs.nix.version == nixVersion;
      pkgs.nix
    )
    (
      assert pkgs.statix.version == statixVersion;
      pkgs.statix
    )
    (
      assert pkgs.cargo-nextest.version == nextestVersion;
      pkgs.cargo-nextest
    )
  ];

  # The flake's inputs as flake.lock locks them (nix/inputs.nix), fetched
  # by this shell outside the gate's sandbox and given to nix:typecheck and
  # nix:test by their store paths, which they read with no network and no
  # nix daemon, so every host evaluates the flake against the inputs the
  # lock pins (core.24, record 0098).
  env.SLUICE_FLAKE_INPUTS = builtins.toJSON (import ./nix/inputs.nix { }).paths;

  env.PLAYWRIGHT_BROWSERS_PATH = "${pkgs.playwright-driver.browsers-chromium}";

  # Claude Code's project files (boomdrive's record 0022; record 0101):
  # .mcp.json from the servers below and .claude/settings.json from the
  # rest, each a copy devenv writes afresh at every shell and readiness, and
  # git ignores. Neither is a link into the store: Claude Code watches a
  # project's settings by the file's real path, so a link had each session
  # watching the store's own directory and reading all of it again whenever
  # a build added or removed a path. On deneb on 2026-11-11 those reads held
  # the store's lock and every nix program hung. The repository's one server
  # is devenv's hosted one, which searches packages and devenv's options and
  # starts no process on the host; it is named here because a list of the
  # repository's own replaces devenv's default. Every server named is
  # approved by name in the settings, so a session in a trusted folder, a
  # place among them, starts them and asks nothing.
  claude.code = {
    enable = true;
    mcpServers."mcp.devenv.sh" = {
      type = "http";
      url = "https://mcp.devenv.sh";
    };
    # Vale on the prose a session writes, in the turn that wrote it
    # (.claude/hooks/vale.sh).
    hooks.vale = {
      name = "Vale on the site's and the docs' prose";
      hookType = "PostToolUse";
      matcher = "Write|Edit";
      command = ''"$CLAUDE_PROJECT_DIR"/.claude/hooks/vale.sh'';
    };
    # devenv's own hook, on wherever git-hooks are, runs prek after every
    # edit: treefmt, and clippy over the workspace once a Rust file is
    # staged, in each turn. The pre-commit hook runs them on what a commit
    # holds, and the gate on what lands.
    hooks.git-hooks-run.enable = false;
  };
  files.${config.claude.code.settingsPath} = {
    json.enabledMcpjsonServers = lib.attrNames config.claude.code.mcpServers;
    copyMode = "copy";
  };
  files.".mcp.json".copyMode = "copy";

  # moon's task archives (.moon/cache/outputs) are a link into the git common
  # dir, so every worktree of sluice shares them: a crate's tests that passed
  # in one worktree replay in another. Where the sandbox or the file system
  # refuses that directory, the worktree keeps its own archives and the shell
  # says nothing: a run writes only its place, its scratch and the caches its
  # host declares (35; record 0049; core.34), so a host that wants them
  # shared declares the directory one of its [[caches]]. The rest of
  # .moon/cache records the checkout's absolute paths and stays its own. The
  # worktree is found when the shell is entered, never written in at
  # evaluation: a printed toolchain is the same from any directory, so sluice
  # can keep one per hash of its inputs.
  enterShell = ''
    # devenv's files, Claude Code's above, made here as well as by devenv's
    # own task: a place's readiness is sluice's equip, which sources this
    # script from `devenv print-dev-env` and runs no task, and the agent
    # started after it reads them as it starts (record 0101). Its words go
    # to no output a caller reads the environment from, and a working copy
    # where they cannot be written still gets its shell.
    if ! (${writeFiles}) >/dev/null; then
      echo "devenv.nix: Claude Code's files were not written in ${config.devenv.root}" >&2
    fi
    if _moon_root="$(git rev-parse --show-toplevel 2>/dev/null)" &&
      _moon_common="$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null)"; then
      _moon_cache="$_moon_root/.moon/cache"
      [ -L "$_moon_cache" ] && rm -f "$_moon_cache"
      mkdir -p "$_moon_cache"
      if mkdir -p "$_moon_common/moon-outputs" 2>/dev/null &&
        [ -w "$_moon_common/moon-outputs" ] &&
        [ ! -L "$_moon_cache/outputs" ]; then
        rm -rf "$_moon_cache/outputs"
        ln -s "$_moon_common/moon-outputs" "$_moon_cache/outputs"
      fi
      # moon puts $MOON_HOME/bin on every task's PATH too: this repository's,
      # beside proto's, so no moon of the user's runs in a task.
      MOON_HOME="$_moon_common/moon-home"
      export MOON_HOME
    fi
    unset _moon_root _moon_common _moon_cache
    # A sluice run is handed its share of the host's cores (SLUICE_JOBS):
    # cargo builds and runs tests with that many, so the runs the host's
    # bound lets go at once share its cores rather than each taking them
    # all. A session's own shell, with no SLUICE_JOBS, keeps every core.
    if [ -n "''${SLUICE_JOBS:-}" ]; then
      CARGO_BUILD_JOBS="$SLUICE_JOBS"
      RUST_TEST_THREADS="$SLUICE_JOBS"
      export CARGO_BUILD_JOBS RUST_TEST_THREADS
    fi
    # In a session's own shell rustc runs under sccache, whose server and
    # cache are the user's, under their home, so the agents' worktrees on
    # one host reuse each other's compiled crates ($RUSTC_WRAPPER
    # --show-stats). A run sluice starts, told by the SLUICE_CHECKOUT it
    # sets, runs rustc unwrapped, and so does CI, told by the CI its
    # runner sets: its cache starts empty, and its gate builds as
    # `sluice exec`'s does (core.24).
    if [ -z "''${SLUICE_CHECKOUT:-}" ] && [ -z "''${CI:-}" ]; then
      RUSTC_WRAPPER="${pkgs.sccache}/bin/sccache"
      export RUSTC_WRAPPER
    fi
    # Only activates: sluice sources this inside its executor, where
    # nothing may be installed.
    ${activate}
  '';

  # The tool versions .prototools lists, installed on entering the shell, before
  # enterShell activates it. A task, not part of enterShell: the script
  # `devenv print-dev-env` prints holds enterShell and none of the tasks,
  # so a check run sourcing it never installs. sluice's moon runner runs
  # `proto install` itself, outside the executor.
  tasks."sluice:proto:install" = {
    exec = ''
      ${protoHome}
      ${proto}/bin/proto install --config-mode upwards
    '';
    before = [ "devenv:enterShell" ];
  };

  treefmt.enable = true;
  treefmt.config = {
    imports = [ ./treefmt.nix ];
    # The tree is found from where treefmt runs, by this file, rather than
    # devenv's evaluated root written into the wrapper: with that, a
    # wrapper evaluated elsewhere checks the other tree.
    projectRootFile = "devenv.nix";
    programs.rustfmt.package = rustfmt;
    # Given no standard input by treefmt, cfn-lint fails reading it.
    settings.formatter.cfn-lint.command = "${pkgs.writeShellScript "cfn-lint" ''
      exec ${cfnLint}/bin/cfn-lint "$@" </dev/null
    ''}";
  };

  # `devenv shell -- cargo-tests`: every crate's cargo tests, through
  # moon, replaying a crate whose inputs already passed.
  scripts.cargo-tests = {
    description = "Every crate's cargo tests: moon run of the tasks tagged cargo-test";
    exec = ''
      set -euo pipefail
      cd "$(git rev-parse --show-toplevel)"
      moon run ':#cargo-test' "$@" </dev/null
    '';
  };

  # `devenv shell -- cargo-tests-affected`: only the crates a change
  # touches, and those depending on them.
  scripts.cargo-tests-affected = {
    description = "The cargo tests of what changed since main: moon run --affected";
    exec = ''
      set -euo pipefail
      cd "$(git rev-parse --show-toplevel)"
      moon run ':#cargo-test' --affected --downstream deep "$@" </dev/null
    '';
  };

  scripts.lint = {
    description = "The lint gate: cargo clippy -D warnings";
    exec = ''exec ${lint}/bin/lint "$@"'';
  };
  # What CI would release from here: the next version and its notes.
  scripts.release-preview = {
    description = "The next release's version and notes: git cliff --unreleased --bump";
    exec = ''
      set -euo pipefail
      cd "$(git rev-parse --show-toplevel)"
      git cliff --unreleased --bump --strip all "$@"
    '';
  };
  scripts.disk = {
    description = "What target/ and the disk have grown to";
    exec = ''exec ${disk}/bin/disk "$@"'';
  };
  scripts.prose = {
    description = "The documentation site's prose, checked by Vale (.vale.ini)";
    exec = ''
      set -euo pipefail
      cd "$(git rev-parse --show-toplevel)"
      vale --glob='!{**/decisions/**,**/architecture/rules.md}' site/src/content/docs "$@"
    '';
  };

  # The floor under every commit, installed into the common hooks directory
  # on `devenv shell`. convco refuses a message that is not a conventional
  # commit (.versionrc), since git-cliff releases from them.
  git-hooks.hooks = {
    convco = {
      enable = true;
      settings.configPath = ".versionrc";
      # A message `git revert` wrote is taken, as CI's `convco check
      # --ignore-reverts` takes its commit. git revert commits without the
      # hook, but a revert it stopped on a conflict or made --no-commit, or
      # one amended, is committed through it. convco reads a revert as a
      # message that opens `Revert "`, and its --from-stdin, the hook's,
      # reads no --ignore-reverts. A revert of a revert, `Reapply "`, is
      # refused by both and reworded.
      entry =
        let
          convco = config.git-hooks.hooks.convco;
        in
        lib.mkForce (
          toString (
            pkgs.writeShellScript "precommit-convco" ''
              IFS= read -r subject < "$1" || true
              case "$subject" in 'Revert "'*) exit 0 ;; esac
              exec ${convco.package}/bin/convco check --from-stdin --config ${convco.settings.configPath} < "$1"
            ''
          )
        );
    };
    treefmt.enable = true;
    clippy-gate = {
      enable = true;
      name = "clippy (-D warnings)";
      entry = "${lint}/bin/lint";
      files = "\\.rs$|Cargo\\.(toml|lock)$";
      pass_filenames = false;
    };
  };

  # devenv 2.4 installs the hook with `prek install -c
  # "$DEVENV_ROOT/.pre-commit-config.yaml"`, and prek writes that absolute
  # path into the hook, which lives in the hooks directory every worktree
  # shares: each worktree's shell would point every commit at its own
  # config (cachix/devenv#3270). The hook installed here is prek's shim
  # reading the config at the top of the worktree the commit is in, as a
  # plain `prek install` does, which devenv links there; in a worktree
  # whose shell has not made that link yet, a new place's, it reads the
  # main checkout's, and with neither it lets the commit through. That
  # fallback is sluice's, so this stays once devenv's fix lands.
  tasks."devenv:git-hooks:install".exec =
    let
      hook =
        type:
        pkgs.writeScript "prek-${type}" ''
          #!/bin/sh
          # sluice's ${type} hook, installed by devenv.nix: prek, with the
          # hook config this worktree's devenv shell links, or the main
          # checkout's before it has.

          HERE="$(cd "$(dirname "$0")" && pwd)"
          PREK="${lib.getExe config.git-hooks.package}"

          # Check if the full path to prek is executable, otherwise fallback to PATH
          if [ ! -x "$PREK" ]; then
              PREK="prek"
          fi

          if [ -e .pre-commit-config.yaml ]; then
              exec "$PREK" hook-impl --hook-dir "$HERE" --script-version 4 --hook-type=${type} -- "$@"
          fi
          # The main checkout: git's main worktree, or in a bare repository
          # the worktree `main` beside it.
          common="$(git rev-parse --path-format=absolute --git-common-dir)"
          if [ "$(git config --bool core.bare)" = true ]; then
              main="''${common%/*}/main"
          else
              main="''${common%/*}"
          fi
          exec "$PREK" hook-impl --hook-dir "$HERE" --script-version 4 --hook-type=${type} \
              --config "$main/.pre-commit-config.yaml" --skip-on-missing-config -- "$@"
        '';
    in
    lib.mkForce ''
      set -euo pipefail
      hooks="$(${lib.getExe pkgs.git} rev-parse --path-format=absolute --git-path hooks)"
      mkdir -p "$hooks"
      ${lib.concatMapStrings
        (type: ''
          cmp -s ${hook type} "$hooks/${type}" || install -m 755 ${hook type} "$hooks/${type}"
        '')
        [
          "pre-commit"
          "commit-msg"
        ]
      }
    '';
}
