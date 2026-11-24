#!/bin/sh
# The plugin's tests under dash, the /bin/sh of Debian and Ubuntu, which
# herdr runs the plugin's commands with there; skipped, visibly, where dash
# is not installed.
set -u
here=$(cd "$(dirname "$0")" && pwd -P)
if ! command -v dash >/dev/null 2>&1; then
  echo "== dash is not installed here; skipped"
  exit 0
fi
SLUICE_HERDR_SH=dash exec sh "$here/groups.sh"
