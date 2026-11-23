# shellcheck shell=sh disable=SC2154,SC2016 # jq programs; common.sh sets lib, state and config.
# sources.toml, read as the subset of TOML the README states: `[[sources]]`
# tables; `key = "string"` with \" and \; one-line arrays of strings;
# true and false; # comments. Prints {"ahead_check", "sources": [...]},
# each source {name, command, folders}; a line outside the subset is
# logged with its number and the default is printed instead. A file that
# names the former key `each_repository` is refused: it prints no source,
# and {"refused": "<the error>"} for the watcher to say (D5). Sourced,
# never run.

default_sources='{"ahead_check":true,"sources":[{"name":"local","command":["sluice","overview","--json","--watch","--stdin"],"folders":true}]}'

read_sources() {
  file=$config/sources.toml
  if [ ! -f "$file" ]; then
    echo "$default_sources"
    return
  fi
  got=$(awk -f "$lib/toml.awk" "$file")
  case $? in
  0) ;;
  2)
    printf '%s\n' "$got" | jq -c '{ahead_check: true, sources: [], refused}'
    return
    ;;
  *)
    echo "$default_sources"
    return
    ;;
  esac
  printf '%s\n' "$got" | jq -s -c -e --argjson d "$default_sources" '
    (map(select(.i == 0)) | map({(.k): .v}) | add // {}) as $top
    | [ group_by(.i)[] | select(.[0].i > 0) | map({(.k): .v}) | add ] as $s
    | { ahead_check: (if $top | has("ahead_check") then $top.ahead_check else true end),
        sources: (if ($s | length) == 0 then $d.sources
                  else $s | map({name: (.name // "source"), command, folders: (.folders == true)})
                    | map(select((.command | type) == "array" and (.command | length) > 0)) end) }' 2>/dev/null ||
    {
      echo "sources.toml: cannot read it as the subset the README states; using the default source" >&2
      echo "$default_sources"
    }
}
