#!/bin/sh
# The cases of check-changes.sh (record 0128 as amended 2026-11-16,
# core.61), which root:changes-cases runs, one for each scenario of
# repository-gate's "Each open change is validated as it would archive":
# each runs sh check-changes.sh over a scratch tree of its own, holding
# OpenSpec's configuration, a spec alpha and the changes the case names,
# written here as data, and reads its exit and its words. It prints each
# failure as one line, changes-cases: <case>: <what>, exits non-zero where
# any case fails, and says every case holds otherwise. Flume's
# check-changes-cases.sh (its record 0073, point 7; core.59) is the
# pattern this one copies, its cases named for this repository's
# scenarios, and the tree read again after a change archive refuses, as
# the scenario has its specs as they were.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
cases=0
failed=0

# A case's tree: OpenSpec's configuration and the spec alpha, a Purpose
# and one requirement, One, with a scenario.
fresh() {
  cases=$((cases + 1))
  tree=$scratch/$cases
  mkdir -p "$tree/openspec/specs/alpha" "$tree/openspec/changes"
  echo 'schema: spec-driven' >"$tree/openspec/config.yaml"
  cat >"$tree/openspec/specs/alpha/spec.md" <<'EOF'
# alpha Specification

## Purpose

The spec alpha, which each case's changes are applied to, holding one requirement.

## Requirements

### Requirement: One

The system SHALL do one thing.

#### Scenario: One is done

- **WHEN** one thing is asked
- **THEN** the system does it
EOF
}

# An open change `$1` in the case's tree: its proposal and its tasks, one
# task unchecked. Its delta of the spec `$2` is read from the input.
change() {
  dir=$tree/openspec/changes/$1
  mkdir -p "$dir/specs/$2"
  cat >"$dir/proposal.md" <<EOF
# $1

## Why

The case's change $1, which check-changes.sh is run over to see what it says.

## What Changes

- The spec $2 changes as the case's delta says.
EOF
  cat >"$dir/tasks.md" <<'EOF'
## 1. The work

- [ ] 1.1 Do the work.
EOF
  cat >"$dir/specs/$2/spec.md"
}

# check-changes.sh run over the case's tree: its exit and its words.
check() {
  status=0
  sh "$here/check-changes.sh" "$tree" >"$tree.out" 2>&1 </dev/null || status=$?
  out=$(cat "$tree.out")
}

# A failure of the case `$1`, saying `$2`, where the test after them fails.
expect() {
  named=$1
  what=$2
  shift 2
  if ! "$@"; then
    echo "changes-cases: $named: $what"
    failed=1
  fi
}

# `$1` holds `$2`.
says() {
  case $1 in *"$2"*) ;; *) return 1 ;; esac
}

# Each file under the case's openspec/ with its checksum, in order.
sums() {
  (cd "$tree" && find openspec -type f | sort | while read -r file; do
    printf '%s %s\n' "$file" "$(cksum <"$file")"
  done)
}

named="An open change that does not validate"
fresh
change adds-two-with-no-scenario alpha <<'EOF'
## ADDED Requirements

### Requirement: Two

The system SHALL do a second thing.
EOF
check
expect "$named" "check-changes.sh exits $status" [ "$status" -ne 0 ]
expect "$named" "it names not the change: $out" says "$out" "adds-two-with-no-scenario"
expect "$named" "it names not Two: $out" says "$out" "Two"

named="A change that adds a requirement its spec holds"
fresh
change adds-one-again alpha <<'EOF'
## ADDED Requirements

### Requirement: One

The system SHALL do one thing again.

#### Scenario: One is done again

- **WHEN** one thing is asked again
- **THEN** the system does it again
EOF
before=$(sums)
check
after=$(sums)
expect "$named" "check-changes.sh exits $status" [ "$status" -ne 0 ]
expect "$named" "it says not that the change would not archive: $out" \
  says "$out" "adds-one-again would not archive"
expect "$named" "it gives not OpenSpec's words that One already exists: $out" \
  says "$out" "already exists"
expect "$named" "a file under openspec/ changed: before $before; after $after" \
  [ "$before" = "$after" ]

named="A change that archives into a spec root:specs would fail"
fresh
change adds-beta beta <<'EOF'
## ADDED Requirements

### Requirement: Beta

The system SHALL do what beta does.

#### Scenario: Beta is done

- **WHEN** beta is asked
- **THEN** the system does it
EOF
check
expect "$named" "check-changes.sh exits $status" [ "$status" -ne 0 ]
expect "$named" "it names not the change and beta: $out" \
  says "$out" "adds-beta would not archive cleanly: the spec beta"
expect "$named" "it gives not OpenSpec's words that the Purpose is a placeholder: $out" \
  says "$out" "placeholder"

named="A modified requirement past OpenSpec's length"
fresh
long="The system SHALL do one thing"
while [ ${#long} -le 500 ]; do
  long="$long, and then one thing more"
done
change makes-one-long alpha <<EOF
## MODIFIED Requirements

### Requirement: One

$long.

#### Scenario: One is done

- **WHEN** one thing is asked
- **THEN** the system does it
EOF
check
expect "$named" "check-changes.sh exits $status" [ "$status" -ne 0 ]
expect "$named" "it names not the change and alpha: $out" \
  says "$out" "makes-one-long would not archive cleanly: the spec alpha"
expect "$named" "it gives not OpenSpec's words that the requirement is very long: $out" \
  says "$out" "very long"

named="A change whose tasks are open"
fresh
for name in adds-two adds-two-too; do
  change "$name" alpha <<'EOF'
## ADDED Requirements

### Requirement: Two

The system SHALL do a second thing.

#### Scenario: Two is done

- **WHEN** a second thing is asked
- **THEN** the system does it
EOF
done
before=$(sums)
check
after=$(sums)
expect "$named" "check-changes.sh exits $status: $out" [ "$status" -eq 0 ]
expect "$named" "it says not that two changes are open and valid: $out" \
  says "$out" "changes: 2 open, each valid"
expect "$named" "a file under openspec/ changed: before $before; after $after" \
  [ "$before" = "$after" ]
expect "$named" "it made $tree/openspec/changes/archive" \
  [ ! -e "$tree/openspec/changes/archive" ]

if [ "$failed" -ne 0 ]; then
  exit 1
fi
echo "changes-cases: every case holds"
