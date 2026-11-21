#!/bin/sh
# Every Rust tool the gate runs is the build .prototools pins, on every
# host (rule core.24): rust-toolchain.toml names the same version, and
# rustc, cargo, clippy and rustfmt each report it. A tool found in another
# toolchain, a runner's rustup default say, fails here rather than holding
# the tree to another version's rules.
set -eu

fail() {
  echo "check-toolchain: $*" >&2
  exit 1
}

pin=$(sed -n 's/^rust *= *"\(.*\)"$/\1/p' .prototools)
[ -n "$pin" ] || fail ".prototools pins no rust"
channel=$(sed -n 's/^channel *= *"\(.*\)"$/\1/p' rust-toolchain.toml)
[ "$channel" = "$pin" ] || fail "rust-toolchain.toml names ${channel:-no channel}, .prototools pins $pin"

rustc=$(rustc -vV)
[ "$(printf '%s\n' "$rustc" | sed -n 's/^release: //p')" = "$pin" ] ||
  fail "rustc is not $pin: $(rustc --version)"
[ "$(cargo -vV | sed -n 's/^release: //p')" = "$pin" ] ||
  fail "cargo is not $pin: $(cargo --version)"

# clippy and rustfmt carry the commit of the rustc they ship with.
commit=$(printf '%s\n' "$rustc" | sed -n 's/^commit-hash: \(.\{9\}\).*/\1/p')
same() {
  case $2 in
  *"($commit"*) ;;
  *) fail "$1 is not $pin's: $2" ;;
  esac
}
same clippy "$(cargo clippy --version)"
same rustfmt "$(rustfmt --version)"
echo "rustc, cargo, clippy and rustfmt are $pin"

# cargo-nextest, which runs the crates' test tasks, is the version
# devenv.nix pins.
nextest=$(sed -n 's/^ *nextestVersion *= *"\(.*\)";$/\1/p' devenv.nix)
[ -n "$nextest" ] || fail "devenv.nix pins no cargo-nextest"
[ "$(cargo nextest --version | sed -n 's/^release: //p')" = "$nextest" ] ||
  fail "cargo-nextest is not $nextest: $(cargo nextest --version | head -n 1)"
echo "cargo-nextest is $nextest"
