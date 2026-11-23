# One line a source printed, read whole by the one jq the reader runs for it
# (D8), with the repository's told list and learned worktrees as it found
# them. Run with -R on the line, and:
#   $hr      the host and repository, tab between, whose files these are,
#            or "" before the reader knows;
#   $told    the told list's text, "" where there is none: a first sight;
#   $paths   the learned worktrees' text.
# Prints `bad` for a line it cannot read as sluice/overview@1, `folder`
# with the line's folder and how its repository stands for a line of a
# source that names its folders (`unset` where sluice is not set up there
# or the repository is gone, which takes the folder off the subscription;
# `none` where nothing is recorded; else `records`), then
# `key` with the document's host and repository, and, where those are
# $hr's and it has records, what to keep and to tell, a line each:
#   doc, paths, told   the document, the learned worktrees and the told list
#   blamed <id> <branch> <tests>
#   conflict <id> <branch> <paths>
#   landed <id> <branch> <worktree> <revision>
# A submission's worktree and a blame's tests come from the document:
# `submissions[].from.path`, `recent_paths` and `recent_failed` (the
# change `the-plugins-hold-one-subscription-each`, D3).
# Fields are tab-separated; no field holds a control character.
include "words";

# What a held place's release printed, kept to its last line with
# anything on it.
def last_line: if type == "string"
  then ([splits("\n") | select(test("\\S"))] | last // "") | sub("^\\s+"; "") | sub("\\s+$"; "")
  else "" end;

# Each run of control characters in a subject made one space (26), before
# control characters go, which would join its lines into one.
def spaced: if type == "string" then gsub("\\p{Cc}+"; " ") else . end;

def release_failed: if (.release_failed | type) == "object" then .release_failed.failed |= last_line else del(.release_failed) end;

# Read as sluice/overview@1 with every list it may leave out made empty;
# anything else is no document.
def normalized:
  select(type == "object" and .schema == "sluice/overview@1" and (.host | type) == "string"
    and (.repository | type) == "string" and ((.branches // []) | type) == "array")
  | .branches = [(.branches // [])[] | select(type == "object" and (.branch | type) == "string")
      | .depth //= 0 | .recent = [(.recent // [])[] | objects]
      | .submissions = [(.submissions // [])[] | objects | select(.id != null) | .from //= {}
          | if has("subject") then .subject |= spaced else . end]
      | if (.recent_subjects | type) == "object" then .recent_subjects |= map_values(spaced) else . end
      | .candidates = [(.candidates // [])[] | objects | .submissions //= [] | .targets //= [] | .failed //= []
          | .tasks = [(.tasks // [])[] | objects | select((.target | type) == "string")]]
      | .held_checkouts = [(.held_checkouts // [])[] | objects | select((.place | type) == "string") | .failed |= last_line]]
  | .sessions = [(.sessions // [])[] | objects | select((.worktree | type) == "string")
      | if (.run | type) == "object" then .run.tasks = [(.run.tasks // [])[] | objects | select((.target | type) == "string")] else del(.run) end
      | if (.last_result | type) == "object" then .last_result.tasks = [(.last_result.tasks // [])[] | objects | select((.target | type) == "string")] else del(.last_result) end
      | release_failed]
  | if (.room | type) == "object" and (.room.bound | type) == "number" then . else del(.room) end
  | .previews = [(.previews // [])[] | if type == "object" then release_failed else . end]
  # Nothing a document says is shown with its control characters: a task or
  # path holding an escape would reach the terminal as one.
  | walk(if type == "string" then gsub("[[:cntrl:]]"; "") else . end);

def row: map(if . == null then "" else tostring end | gsub("[[:cntrl:]]"; "")) | join("\t");

# Whether a document holds anything to show: a branch or a session.
def records: (.branches | length) + (.sessions | length) > 0;

(try fromjson catch null) as $raw
| ($raw | if type == "object" and (.folder | type) == "string" then .folder | gsub("[[:cntrl:]]"; "") else null end) as $folder
| ($raw | [normalized] | first) as $d
# A folder sluice is not set up in, or whose repository is gone: no
# document to keep (D4).
| if $folder != null and ($raw.set_up == false or $raw.gone == true) then ["folder", $folder, "unset"] | row
  elif $d == null then "bad"
  else
    (select($folder != null) | ["folder", $folder, (if $d | records then "records" else "none" end)] | row),
    (["key", $d.host, $d.repository] | row),
    (select("\($d.host)\t\($d.repository)" == $hr and ($folder == null or ($d | records)))
      # Each submission's worktree and branch, kept while its id is recent
      # or queued; a finished one leaves submissions[] but stays in recent[].
      | ([$d.branches[].recent[] | (.id // empty), (.ids // [])[]] + [$d.branches[].submissions[].id]
          | map(tostring) | map({(.): true}) | add // {}) as $live
      | (($paths | try fromjson catch {}) | if type == "object" then . else {} end) as $known
      | ($known
          + ([$d.branches[] | .branch as $b | (.recent_paths | if type == "object" then . else {} end)
              | to_entries[] | select(.value | type == "string") | {(.key): {path: .value, branch: $b}}] | add // {})
          + ([$d.branches[] | .branch as $b | .submissions[] | select(.from.path | type == "string")
              | {(.id | tostring): {path: .from.path, branch: $b}}] | add // {})
        | with_entries(select($live[.key]))) as $learned
      # The events told, by their keys, while they are among the recent; a
      # list missing or unreadable is a first sight, which tells nothing.
      | (if $told == "" then null else ($told | try fromjson catch null) end
          | if type == "array" then map({(tostring): true}) | add // {} else null end) as $was
      | [$d | events] as $events
      | ("doc\t" + ($d | tojson)),
        ("paths\t" + ($learned | tojson)),
        ("told\t" + ([$events[].key] | unique | tojson)),
        (select($was != null) | $events[] | select($was[.key] | not)
          | .branch as $b | .event as $e
          | if $e.event == "blamed" then
              ($e.id | tostring) as $i
              | ([$d.branches[] | select(.branch == $b) | .recent_failed | objects | .[$i]] | first) as $f
              | ["blamed", $i, $b, ($f | if type == "array" then map(tostring) | join(", ") else "" end)] | row
            elif $e.event == "conflict" then
              ($e.id | tostring) as $i
              | ["conflict", $i, $b, (($e.paths // []) | map(tostring) | join(", "))] | row
            elif $e.event == "landed" then
              ($e.ids // [])[] | tostring as $i | select($learned[$i].path)
              | ["landed", $i, $b, $learned[$i].path, (($e.to // "") | tostring | .[0:7])] | row
            else empty end))
  end
