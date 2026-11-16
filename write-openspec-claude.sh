#!/bin/sh
# Writes OpenSpec's Claude Code skills and commands, which the tree tracks
# under .claude/skills/openspec-* and .claude/commands/opsx, with the
# OpenSpec devenv.nix pins (records 0101, 0127; core.24): ten workflows,
# propose, explore, new, continue, apply, ff, sync, archive, bulk-archive
# and verify, as skills and as commands. They are written afresh into a
# scratch directory under a configuration of this script's own, never the
# user's, and replace the tracked ones. Moving the pin asks for it; no task
# runs it, since no task reads them (principle 16; core.21).
set -eu

fail() {
  echo "write-openspec-claude: $*" >&2
  exit 1
}

[ $# -eq 0 ] || fail "usage: sh write-openspec-claude.sh"

pin=$(sed -n 's/^ *openspecVersion = "\(.*\)";$/\1/p' devenv.nix)
[ -n "$pin" ] || fail "devenv.nix pins no openspecVersion"
OPENSPEC_TELEMETRY=0
OPENSPEC_NO_UPDATE_CHECK=1
export OPENSPEC_TELEMETRY OPENSPEC_NO_UPDATE_CHECK
found=$(openspec --version)
[ "$found" = "$pin" ] ||
  fail "openspec is $found, devenv.nix pins $pin: run this in devenv shell"

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/config/openspec" "$scratch/tree"
cat >"$scratch/config/openspec/config.json" <<'EOF'
{
  "profile": "custom",
  "delivery": "both",
  "workflows": ["propose", "explore", "new", "continue", "apply", "ff", "sync", "archive", "bulk-archive", "verify"]
}
EOF
XDG_CONFIG_HOME="$scratch/config" openspec init --tools claude --profile custom \
  --no-animation "$scratch/tree" </dev/null >/dev/null ||
  fail "openspec $pin could not write its Claude Code files"
made="$scratch/tree/.claude"

rm -rf .claude/commands/opsx .claude/skills/openspec-*
mkdir -p .claude/commands .claude/skills
cp -R "$made/commands/opsx" .claude/commands/
cp -R "$made/skills/"openspec-* .claude/skills/
echo "OpenSpec $pin's Claude Code skills and commands written under .claude"
