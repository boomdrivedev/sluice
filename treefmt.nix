_: {
  # A tracked file no formatter claims and no exclude names fails the check
  # (core.21): a new file type is decided in the change that adds it.
  settings.on-unmatched = "fatal";

  # What no formatter owns, each with its reason (core.21).
  settings.excludes = [
    # Prose: the site's pages are Vale's (site:prose), and no formatter
    # here rewraps prose.
    "*.md"
    "*.mdx"
    # Written by cargo, devenv and bun in their own layouts.
    "*.lock"
    # Records as sluice writes them, kept byte for byte.
    "*.jsonl"
    "docs/examples/**"
    # Drawn images, rendered diagrams and the plugins' screenshots, captured
    # terminal frames kept as shot; no formatter keeps them stable.
    "*.svg"
    "*.png"
    # Test keys, kept byte for byte.
    "*.pem"
    # Sigstore bundles recorded as their signers' tools wrote them, kept
    # byte for byte (record 0119; design D13).
    "crates/sluice/tests/profiles/forge/*.sigstore.json"
    # The page the social image is rendered from, kept as drawn; Biome's
    # HTML support is experimental.
    "docs/brand/social-preview.html"
    # Vale's own configuration and word lists, which no formatter reads.
    ".vale.ini"
    "styles/config/vocabularies/**"
    # A list in gitignore syntax, and empty markers.
    ".worktreeinclude"
    "*.gitkeep"
    # The site's components: Biome formats only an .astro file's
    # frontmatter, and only experimentally.
    "site/**/*.astro"
    # Build outputs and caches.
    "target/**"
    ".devenv/**"
    ".moon/cache/**"
    # The engine's own declarations, copied whole from a session of the
    # pinned Claude Code (plugins/sluice-board/README.md).
    "plugins/sluice-board/engine-types/**"
    # The herdr plugin's fixture documents and frames, compared byte for
    # byte.
    "plugins/herdr/tests/docs/**"
    # The module's rendered files, compared byte for byte by the flake's
    # cases; taplo would rewrite them.
    "nix/tests/files/**"
    # The herdr plugin's awk and jq programs: no formatter or linter for
    # either in the toolchain; plugin-herdr's tests run each under sh and
    # dash, and a parse error fails it.
    "plugins/herdr/lib/*.awk"
    "plugins/herdr/lib/*.jq"
  ];

  programs.rustfmt.enable = true;
  programs.rustfmt.edition = "2021";

  programs.deadnix.enable = true;
  programs.nixfmt.enable = true;
  settings.formatter.deadnix.pipeline = "nix";
  settings.formatter.deadnix.priority = 1;
  settings.formatter.nixfmt.pipeline = "nix";
  settings.formatter.nixfmt.priority = 2;

  programs.shellcheck.enable = true;
  programs.shfmt.enable = true;
  programs.shfmt.indent_size = 2;
  settings.formatter.shellcheck.pipeline = "shell";
  settings.formatter.shellcheck.priority = 1;
  settings.formatter.shfmt.pipeline = "shell";
  settings.formatter.shfmt.priority = 2;
  # The herdr plugin's stand-ins: #!/bin/sh scripts with no extension.
  settings.formatter.shellcheck.includes = [
    "plugins/herdr/tests/fake/*"
    "plugins/herdr/tests/fake-*/*"
  ];
  settings.formatter.shfmt.includes = [
    "plugins/herdr/tests/fake/*"
    "plugins/herdr/tests/fake-*/*"
  ];

  # .prototools is TOML and .versionrc convco's YAML, under names with no
  # extension.
  programs.taplo.enable = true;
  settings.formatter.taplo.includes = [ ".prototools" ];
  programs.yamlfmt.enable = true;
  settings.formatter.yamlfmt.includes = [ ".versionrc" ];
  programs.yamlfmt.settings.formatter = {
    type = "basic";
    retain_line_breaks_single = true;
  };

  # Biome formats and lints the TypeScript, JavaScript, CSS and JSON in
  # one pass (`biome check`), its recommended rules and nothing more; a
  # lint error fails `treefmt --ci`. Its settings are here, not in a
  # biome.json, since treefmt-nix hands Biome a generated one.
  programs.biome.enable = true;
  programs.biome.formatCommand = "check";
  programs.biome.settings = {
    formatter = {
      indentStyle = "space";
      indentWidth = 2;
      lineWidth = 100;
    };
    javascript.formatter = {
      quoteStyle = "double";
      trailingCommas = "all";
      semicolons = "always";
    };
    json.formatter.trailingCommas = "none";
    linter.rules.recommended = true;
  };
  # actionlint reads the workflows' keys and expressions, which yamlfmt
  # only lays out.
  programs.actionlint.enable = true;
  # The workflows `sluice init actions` writes, from their templates
  # (record 0119; design D11).
  settings.formatter.actionlint.includes = [
    ".github/workflows/*.yml"
    ".github/workflows/*.yaml"
    "crates/sluice/src/profiles/github/workflows/*.yml"
  ];

  # cfn-lint reads the CodeBuild pool's template's resources, properties
  # and functions, which yamlfmt only lays out (record 0119; design D10 of
  # the change `a-codebuild-fleet-runs-a-queues-checks`); its command is
  # the build devenv.nix pins.
  settings.formatter.cfn-lint.includes = [
    "crates/sluice/src/profiles/codebuild/templates/*.yml"
  ];

  # ruff lints and formats the diagram scripts: its default rules, then
  # its format.
  programs.ruff-check.enable = true;
  programs.ruff-format.enable = true;
  settings.formatter.ruff-check.pipeline = "python";
  settings.formatter.ruff-check.priority = 1;
  settings.formatter.ruff-format.pipeline = "python";
  settings.formatter.ruff-format.priority = 2;

}
