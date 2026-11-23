# What the plugin says, from sluice/overview@1 documents: the same words as
# plugins/sluice-board/hooks/board.ts, so a blame reads the same in both.
# The popup and the segment name each submission by its subject, or its
# branch, beside its number (26), where the board does not yet.

# A repository's name, as name_of in common.sh gives it.
def repo_name: sub("/\\.git$"; "") | split("/") | last | sub("\\.git$"; "");

def pad2: tostring | if length < 2 then "0" + . else . end;

# `2m10s`, `45s`, `1h03m`.
def duration: (if . < 0 then 0 else . end | floor) as $s
  | if $s < 60 then "\($s)s"
    elif $s < 3600 then "\($s / 60 | floor)m\($s % 60 | pad2)s"
    else "\($s / 3600 | floor)h\(($s % 3600) / 60 | floor | pad2)m" end;

# `15m` for a limit: whole minutes where it divides.
def limit: if . % 60 == 0 and . < 3600 then "\(. / 60)m" else duration end;

# Seconds since an RFC 3339 time, any fraction of a second dropped.
def since($now): if . == null then null
  else (sub("\\.[0-9]+"; "") | try fromdateiso8601 catch null) as $t
    | if $t == null then null else $now - $t end end;

def plural($n; $word): "\($n) \($word)\(if $n == 1 then "" else "s" end)";

# --- A submission's name ----------------------------------------------

# A name on one line: each run of control characters one space, the ends
# trimmed, as sluice's own text shows it.
def one_line: gsub("\\p{Cc}+"; " ") | sub("^\\s+"; "") | sub("\\s+$"; "");

# The name submission . of the branch $b is shown by (26): one in its
# chain by its subject, else its branch; a recent one by recent_subjects,
# else recent_from; null where the document gives neither, as an older
# sluice's does.
def name_of($b): . as $id | tostring as $k
  | (first($b.submissions[]? | select(.id == $id) | .subject // .from.worktree)
      // ($b.recent_subjects // {})[$k] // ($b.recent_from // {})[$k])
  | if type == "string" then one_line | select(. != "") else empty end;

# `#12 Batch review cards`, or `#12` with no name.
def named($b): "#\(.)" + ([name_of($b)] | if length > 0 then " " + .[0] else "" end);

# A line from its pieces, strings and submissions' numbers, each
# submission named the first time the line shows it and by its number
# alone after (26).
def said($b): reduce .[] as $p ({said: {}, text: ""};
    if ($p | type) == "number" then ($p | tostring) as $k
      | if .said[$k] then .text += "#\($p)" else .text += ($p | named($b)) | .said[$k] = true end
    else .text += $p end) | .text;

# Lists of pieces as one, $sep between each.
def joined($sep): reduce .[] as $p (null; if . == null then $p else . + [$sep] + $p end) // [];

# `main + #12 Batch review cards + #13 forecast`: the numbers of a
# combination's submissions on the branch $b, as pieces, and as a line.
def combination_pieces($b): [$b.branch] + map(" + ", .);
def combination($b): combination_pieces($b) | said($b);

# --- A combination's tasks --------------------------------------------

def ended: .state == "passed" or .state == "failed" or .state == "replayed";

# Ten cells, one filled for each tenth of the tasks ended, rounded down.
def bar($done; $total): (if $total > 0 then (10 * ([$done, $total] | min) / $total | floor) else 0 end) as $full
  | ("▓" * $full // "") + ("░" * (10 - $full) // "");

# `3/5 ▓▓▓▓▓▓░░░░`: how many of a combination's tasks have ended, or null
# when its runner says nothing.
def progress: if (.tasks // []) | length == 0 then null
  else ([.tasks[] | select(ended)] | length) as $done
    | "\($done)/\(.tasks | length) \(bar($done; .tasks | length))" end;

def at_seconds: if . == null then null else sub("\\.[0-9]+"; "") | try fromdateiso8601 catch null end;

# What follows a task's name: how long it took once ended, else its state.
def task_detail: if .state == "replayed" or .state == "running" or .state == "waiting" then .state
  else ((.ended | at_seconds) as $e | (.started | at_seconds) as $s
    | if $e != null and $s != null then $e - $s | duration else .state end) end;

def pad_end($n): . + (" " * ($n - length) // "");
def pad_start($n): (" " * ($n - length) // "") + .;

# The task list: ended tasks in the order they ended, then running, then
# waiting, each {state, text} as the board's pane lists them.
def task_lines: (.tasks // []) | to_entries
  | map(.value as $t | {t: $t, key: [
      (if $t | ended then 0 elif $t.state == "running" then 1 else 2 end),
      (if $t | ended then ($t.ended | at_seconds) // 0 else 0 end),
      .key]})
  | sort_by(.key) | map(.t)
  | ([.[].target | length] | max // 0) as $name
  | ([.[] | task_detail | length] | max // 0) as $detail
  | map({state, text: "\({passed: "✓", replayed: "✓", failed: "✗", running: "…", waiting: "·"}[.state] // "·") \(.target | pad_end($name))  \(task_detail | pad_start($detail))"});

def testing: .candidates | map(select(.state == "testing" or .state == "starting"));

def waiting: (testing | map(.submissions[]) ) as $under
  | [.submissions[].id | select(. as $i | $under | index($i) | not)];

def short: if type == "string" then .[0:7] else "" end;

# What a recent log line of the branch $b says, in a few words, as pieces,
# and as a line.
def recent_pieces: if .event == "landed" then ((.ids // []) | [.[] | ", ", .][1:]) + [" landed as \(.to | short)"]
  elif .event == "blamed" then [.id, " blamed on \(.combination | short)"]
  elif .event == "conflict" then [.id, " conflict · \((.paths // []) | join(", "))"]
  else [.event | tostring] end;
def recent_words($b): recent_pieces | said($b);

# The last part of a worktree's path, its name in a row.
def base_name: sub("/+$"; "") | split("/") | last;

# --- Places held because their release failed --------------------------

# Every place of the document $d held because its release failed and tried
# no more (123), each {key, branch, name, release, tries, failed, host}:
# its branches' checkouts, then its worktrees and its previews. A checkout
# is keyed by its name, since every host's document lists it from the
# branch's state; a worktree or a preview is its own host's. One made by
# another host names it in $host, where alone it is released.
def held_all($d):
  [ ($d.branches[] | .branch as $b | (.held_checkouts // [])[] | select(.stopped == true)
      | {key: "checkout \(.place)", branch: $b, name: .place, release: .place, tries, failed,
         host: (if .host != null and .host != $d.host then .host else null end)}),
    ($d.sessions[] | select(.release_failed.stopped == true)
      | {key: "worktree \($d.host) \(.worktree)", branch, name: "worktree \(.worktree | base_name)",
         release: "--worktree \(.worktree)", tries: .release_failed.tries, failed: .release_failed.failed, host: null}),
    ($d.previews[] | objects | select(.release_failed.stopped == true)
      | {key: "preview \($d.host) \(.name)", branch, name: "preview \(.name)", release: .name,
         tries: .release_failed.tries, failed: .release_failed.failed, host: null}) ];

def held_places($d; $branch): held_all($d) | map(select(.branch == $branch));

# The branches of the document's held places that it lists no records of.
def unlisted_held($d): [held_all($d)[].branch] | unique
  | map(select(. as $b | $d.branches | all(.branch != $b)));

def held_words($n): "\($n) held: release failed";

def times: if . == 1 then "once" else "\(.) times" end;

# A held place's two lines, as the board's pane words them: what failed,
# then how to run its release again or free it without.
def held_lines: [
    "✗ \(.name) held\(if .host then " on \(.host)" else "" end) · failed \(.tries | times)\(if (.failed // "") != "" then " · \(.failed)" else "" end)",
    "\(if .host then "on \(.host): " else "" end)sluice release \(.release) · --accept-failure frees it without" ];

# Whether a branch has anything queued or under test; the rest are left out
# of the band.
def busy: .depth > 0 or (testing | length) > 0;

# Whether the popup shows a branch: one busy, or whose last event is under
# a quarter of an hour old, so a host with many idle branches shows its
# queues and a landing is still seen once its queue is empty; or one with
# $held places held because their release failed.
def shown($now): busy or ((.recent | last | .at | since($now)) // 900) < 900;
def shown($now; $held): shown($now) or $held > 0;

# --- Agents' checks ---------------------------------------------------

# Whether agents' checks hold back this branch's queue, by the document's
# $room: the host has no free run, a session run holds one, and the branch
# has a submission queued while fewer of its combinations are under test
# than its window. One whose combination passed waits on no run. Nothing
# without a room: an older sluice gives none.
def held_back($room): $room != null and ($room.sessions // 0) > 0
  and (($room.queue // 0) + $room.sessions) >= ($room.bound // 0)
  and any(.submissions[]; .state == "queued") and (testing | length) < (.window // 0);

def checks_words($room): if $room.sessions == 1 then "1 slot to an agent's checks"
  else "\($room.sessions) slots to agents' checks" end;

# The band's line for one branch, or null when it has nothing to show,
# saying so at its end when agents' checks hold its queue back, and last
# how many of its places are held because their release failed.
def branch_line($now; $room; $held): . as $b | testing as $under
  | if ($b | busy | not) and $held == 0 then null
    else [["sluice \($b.branch)"], [plural($b.depth; "submission")]]
      + ($under | map((.started | since($now)) as $took
          | ["testing "] + (.submissions | combination_pieces($b))
            + [(progress | if . then " " + . else "" end)
              + (if $took == null then "" else " \($took | duration)" + (if .limit then "/\(.limit | limit)" else "" end) end)]))
      + ($b | waiting | if length > 0 then [[.[] | ", ", .][1:] + [" waiting"]] else [] end)
      + ($b.recent | if length > 0 then [["last "] + (.[-1] | recent_pieces)] else [] end)
      + (if $b | held_back($room) then [[checks_words($room)]] else [] end)
      + (if $held > 0 then [[held_words($held)]] else [] end)
      | joined(" · ") | said($b) end;

# A branch the document lists no records of, as one with nothing queued.
def no_records: {branch: ., depth: 0, window: 0, candidates: [], submissions: [], recent: []};

# The tab-bar segment: every branch with a queue or a held place, on one line.
def segment($now): [.[] | . as $d | .room as $room
  | ($d.branches + (unlisted_held($d) | map(no_records)))[]
  | branch_line($now; $room; held_places($d; .branch) | length) | select(. != null)] | join("  |  ");

def paint($on; $code): if $on then "\u001b[\($code)m\(.)\u001b[0m" else . end;

def pull_of: (capture("^pull/(?<n>[0-9]+)$") | .n) // null;

# The rows under a combination under test: its tasks, each painted by its state.
def candidate_rows($c; $color): $c | task_lines
  | map(.state as $s | "        \(.text)" | paint($color; {passed: "32", replayed: "32", failed: "31", running: "33", waiting: "2"}[$s] // "2")
    | {text: ., kind: "task", state: $s});

# How a finished check ended, from its last_result: what its passes are
# worth when its head is submitted, or how many of its tasks failed. A run
# that listed no tasks, a command's, says only how it ended.
def result_words: (.tasks // []) as $t
  | ([$t[] | select(.state == "passed" or .state == "replayed")] | length) as $p
  | ([$t[] | select(.state == "failed")] | length) as $f
  | if $f > 0 then "\($f) of \($t | length) failed"
    elif .outcome != "pass" then {fail: "failed", timed_out: "timed out", lost: "lost", stopped: "stopped", not_run: "not run"}[.outcome] // (.outcome | tostring)
    elif ($t | length) == 0 then "passed"
    else "\($p) passed" + (if .reused then " · reused when submitted"
      elif .kind == "checks" and (.recorded | not) then " · not kept: uncommitted changes"
      else "" end) end;

# The CHECKING rows: each worktree with a check under way, or one that ended
# within a quarter of an hour, as load on the host, never as a submission.
def check_rows($now; $color): [ .sessions[]
  | select(.run or (((.last_result.at | since($now)) // 900) < 900))
  | "    \(.worktree | base_name) → \(.branch) " as $who
  | if .run then .run
      | ((.started | since($now)) // 0 | duration) as $took
      | {text: ($who + (progress // .command // "" | if . == "" then "" else . + " " end) + $took), kind: "check"}
    else .last_result | (if .outcome == "pass" then "32" else "31" end) as $c
      | {text: ($who + result_words | paint($color; $c)), kind: "check"} end ];

# `host: 4 of 6 runs · 3 queue · 1 agent`: the runs counted against the
# host's bound, or null while none is.
def host_line: .room as $r
  | if $r == null or (($r.queue // 0) + ($r.sessions // 0)) == 0 then null
    else (["host: \(($r.queue // 0) + ($r.sessions // 0)) of \($r.bound) runs"]
      + (if ($r.queue // 0) > 0 then ["\($r.queue) queue"] else [] end)
      + (if ($r.sessions // 0) > 0 then [plural($r.sessions; "agent")] else [] end)) | join(" · ") end;

# The branch named for a pane ($named, or null) first, then the rest in
# their order.
def named_first($named): map(select(.branch == $named)) + map(select(.branch != $named));

# One document's rows, as the board's /sluice pane draws them, under the
# headings RELEASE FAILED, TESTING, QUEUED and RECENT: {text, kind, url?}. A
# row with a url is a link: a submission to its worktree's pane, a failed
# test to its log. The branch named for the focused pane ($named, or null)
# comes first, shown when idle, and as one line when no record names it; a
# branch with no records that has a held place follows the branches.
def rows($now; $color; $named): . as $d
  | ($d | check_rows($now; $color)) as $checks | ($d | host_line) as $host
  | def row($kind): {text: ., kind: $kind};
  # A held place is the operator's to release: its rows are no links.
  def held_rows($branch): held_places($d; $branch)
    | if length == 0 then empty
      else ("  RELEASE FAILED" | paint($color; "1") | row("heading")),
        (.[] | held_lines | ("    " + .[0] | paint($color; "31") | row("held")), ("        " + .[1] | paint($color; "2") | row("held")))
      end;
  def visible: shown($now; held_places($d; .branch) | length) or .branch == $named;
  def no_records_rows: ("" | row("blank")), ("\(.) · no records" | paint($color; "1") | row("heading")), held_rows(.);
  [("\($d.repository) on \($d.host)" | paint($color; "2") | row("heading"))]
  + [ if $named != null and ($d.branches | all(.branch != $named)) then $named | no_records_rows else empty end ]
  + (if ([$d.branches[] | select(visible)] | length) == 0 then ["No branch has a queue." | paint($color; "2") | row("heading")] else [] end)
  + [ $d.branches | named_first($named)[] | select(visible) | . as $b | testing as $under
      | ("" | row("blank"))
      , ("\($b.branch) at \(($b.at // "nothing") | .[0:7]) · \(plural($b.depth; "submission")) · \(plural($under | length; "combination")) under test\(if $b.parallel then " (up to \($b.parallel) at once)" else "" end)" | paint($color; "1") | row("heading"))
      , held_rows($b.branch)
      , ( ($under + ($b.candidates | map(select(.state == "formed")))) as $shown
          | if ($shown | length) > 0 then
              ("  TESTING" | paint($color; "1") | row("heading"))
              , ($under[] | . as $c | (.started | since($now)) as $took
                  | ("    ▶ testing \(.submissions | combination($b))"
                      + (progress | if . then "  " + . else "" end)
                      + (if $took == null then "" else "  \($took | duration)" + (if .limit then " / \(.limit | limit)" else "" end) end) | paint($color; "33") | row("candidate"))
                  , ("        \(.targets | join(" "))\(if .executor then " · \(.executor)" else "" end)" | paint($color; "2") | row("candidate"))
                  , candidate_rows($c; $color)[])
              , ($b.candidates[] | select(.state == "formed")
                  | "        formed, not running: \(.submissions | combination($b))" | paint($color; "2") | row("candidate"))
            else empty end )
      , ( if ($b.submissions | length) > 0 then
            ([$b.submissions[].id | named($b) | length] | max) as $width
            | ("  QUEUED" | paint($color; "1") | row("heading"))
            , ($b.submissions[] | (.from.worktree | pull_of) as $pull
                | ("    \(.id | named($b) | pad_end($width))  \(.state + "         " | .[0:9]) \(if $pull then "PR \($pull)" else .from.worktree end) \(.from.agent)" | row("submission"))
                  + (if .from.path then {url: "sluice-herdr:focus?path=\(.from.path | @uri)"} else {} end))
          else empty end )
      , ( if ($b.recent | length) > 0 then
            ("  RECENT" | paint($color; "1") | row("recent"))
            , ($b.recent | reverse | .[0:5] | to_entries[] | .key as $age | .value
                | (if .event == "landed" then ["✓", "32"] elif .event == "blamed" then ["✗", "31"] else ["⚠", "33"] end) as [$mark, $c]
                | "    \($mark) \(.at[11:16]) \(recent_words($b))" | paint($color; $c) | row("event") + {age: $age})
            , ([$b.candidates[] | select(any(.failed[]; .log))] | last // empty
                | .failed[] | select(.log)
                | ("      log of \(.task)\(if .system then " (\(.system))" else "" end): \(.log)" | row("failure"))
                  + {url: "sluice-herdr:log?path=\(.log | @uri)&task=\(.task | @uri)"})
          else empty end )
    ]
  + [ unlisted_held($d)[] | select(. != $named) | no_records_rows ]
  + ([$d.branches[] | select(visible | not)] | length | if . > 0 and . < ($d.branches | length)
      then [("" | row("blank")), ((if . == 1 then "1 other branch" else "\(.) other branches" end) + " with nothing queued" | paint($color; "2") | row("heading"))] else [] end)
  + (if ($checks | length) > 0 or $host then [("" | row("blank"))] else [] end)
  + (if ($checks | length) > 0 then [("CHECKING" | paint($color; "1") | row("heading"))] + $checks else [] end)
  + (if $host then [("  " + $host | paint($color; "2") | row("heading"))] else [] end)
  # A worktree held is one sluice gave ports; one that only has a branch
  # sluice lands on checked out holds nothing.
  + [("" | row("blank")), ("\(plural([$d.sessions[] | select(.ports)] | length; "worktree")) held · \($d.previews | length) previews" | paint($color; "2") | row("heading"))];

# --- Fitting rows to a height -----------------------------------------

# Each combination's task list cut to its running and failed tasks, the
# rest counted on one line: `… 18 passed, 4 waiting`.
def collapse_tasks($color):
  def kept: .state == "running" or .state == "failed";
  def counted: map(select(kept | not) | .state | if . == "passed" or . == "replayed" then . else "waiting" end) as $gone
    | if ($gone | length) == 0 then []
      else [{kind: "task", state: "counted", text: ("        … " + ([("passed", "replayed", "waiting") as $k
          | ($gone | map(select(. == $k)) | length) as $n | select($n > 0) | "\($n) \($k)"] | join(", ")) | paint($color; "2"))}] end;
  reduce (.[], {kind: "end"}) as $r ({out: [], tasks: []};
    if $r.kind == "task" then .tasks += [$r]
    else .out += (.tasks | map(select(kept))) + (.tasks | counted) + (if $r.kind == "end" then [] else [$r] end)
      | .tasks = [] end)
  | .out;

# Rows in at most $height lines, or all of them when $height is null: the
# task lists collapsed first, then a window of them from row $top on, with
# a line saying how many lines are above it and one how many below. The
# line above carries the window's top, kept within the rows, for the next
# scroll to start from.
def fit($height; $top; $color):
  if $height == null or length <= $height then .
  else ([$height, 3] | max) as $h
    | collapse_tasks($color)
    | if length <= $h then .
      else length as $n
        | ([[$top, 0] | max, $n - $h + 1] | min) as $t
        | if $t == 0 then .[0:$h - 1] + [{kind: "more", text: ("… \($n - $h + 1) more lines · ↓ scrolls" | paint($color; "2"))}]
          else [{kind: "above", top: $t, text: ("… \($t) lines above · ↑ scrolls" | paint($color; "2"))}]
            + if $t + $h - 1 >= $n then .[$t:]
              else .[$t:$t + $h - 2] + [{kind: "more", text: ("… \($n - $t - $h + 2) more lines" | paint($color; "2"))}] end end end end;

# Rows with a url numbered 1 to 9, in the order drawn.
def numbered: reduce .[] as $r ({n: 0, out: []};
    if $r.url and .n < 9 then .n += 1 | .out += [$r + {n: .n}] else .out += [$r] end) | .out;

# A row as a line: its number in the margin its indent leaves, and, with
# $links, its text an OSC 8 link to its url.
def line($links):
  (if .n then "[\(.n)]" + (.text | .[3:]) else .text end) as $t
  | if $links and .url then "\u001b]8;;\(.url)\u001b\\\($t)\u001b]8;;\u001b\\" else $t end;

# --- The sidebar -------------------------------------------------------

def minutes: "\(. / 60 | floor)m";

# The state of the branch $branch, named for a pane whose worktree can't
# say it, from its repository's documents: `release · 3 queued · #12
# testing 3/8 · 2 checking`, its oldest combination under test and the
# agents' checks held to it; `release · nothing queued`, or `release · no
# records` when no document lists it. The board's status line says the same.
def named_line($branch; $now):
  . as $docs
  | ([$docs[].branches[] | select(.branch == $branch)] | first) as $b
  | if $b == null then "\($branch) · no records"
    else ($b | testing) as $under
      | ($b | waiting | length) as $queued
      | ([$docs[].sessions[] | select(.branch == $branch and .run)] | length) as $checking
      | [$branch]
        + (if $queued > 0 then ["\($queued) queued"] else [] end)
        + (if ($under | length) > 0 then
            [($under | min_by(.started // "~")) as $c
              | "\($c.submissions | map("#\(.)") | join("+")) testing"
                + (if ($c.tasks // []) | length > 0 then " \([$c.tasks[] | select(ended)] | length)/\($c.tasks | length)"
                  else ($c.started | since($now)) as $took | if $took == null then "" else " \($took | minutes)" end end)
                + (if ($under | length) > 1 then " +\(($under | length) - 1) more" else "" end)]
          else [] end)
        + (if $checking > 0 then ["\($checking) checking"] else [] end)
      | if length == 1 then . + ["nothing queued"] else . end
      | join(" · ") end;

# The `sluice` token for the worktree $w, from its repository's documents,
# the submissions learned for it ($paths: id -> {path, branch}), its last
# forecast ($check) and the branch named for its pane ($named, or null). A
# blame's tests are its branch's `recent_failed`.
def pane_token($w; $paths; $check; $now; $named):
  . as $docs
  | ($paths | to_entries | map(select(.value.path == $w) | .key | tonumber)) as $mine
  | [ $docs[].branches[] | . as $b | .submissions[] | select(.from.path == $w)
      | . as $s | ($b | testing | map(select(.submissions | index($s.id))) | first) as $c
      | if $c then "#\($s.id) testing" + ($c | progress | if . then " " + . else "" end)
          + (($c.started | since($now)) as $took | if $took == null then "" else " \($took | minutes)" + (if $c.limit then "/\($c.limit | minutes)" else "" end) end)
        else "#\($s.id) \($s.state)" end ] as $live
  | [ $docs[].branches[] | (.recent_failed | objects // {}) as $f | .recent[]
      | select((.event == "blamed" or .event == "conflict") and (.id as $i | $mine | index($i)))
      | . as $r | select($mine | all(. <= $r.id)) | . + {failed: $f[.id | tostring]} ] | sort_by(.at) | last as $out
  | [ $docs[].branches[].recent[] | select(.event == "landed")
      | select(any(.ids[]; . as $i | $mine | index($i)))
      | {at, id: ([.ids[] | . as $i | select($mine | index($i))] | max)} ] | sort_by(.at) | last as $landed
  | [ $docs[].sessions[] | select(.worktree == $w and .run) | .run ] | first as $run
  | if ($live | length) > 0 then $live[0]
    elif $out and (($landed == null) or ($landed.id < $out.id)) then
      # Only a task with a plain name fits the line: a command runner's
      # task is its whole command, which the popup still shows.
      if $out.event == "blamed" then "#\($out.id) BLAMED" + (($out.failed | arrays // []) | map(strings | select(test("^\\S{1,24}$")))
          | if length > 0 then " · " + join(", ") else "" end)
      else "#\($out.id) CONFLICT" + (($out.paths // []) | if length > 0 then " · " + .[0] else "" end) end
    # A worktree several agents share says nothing of each one's branch.
    elif $named != null then $docs | named_line($named; $now)
    elif $run then "checking " + ($run | if (.tasks // []) | length > 0
        then "\([.tasks[] | select(ended)] | length)/\(.tasks | length)"
        else (.started | since($now)) // 0 | minutes end)
    elif $check != null and ($check.count // 0) > 0 then
      "\($check.count) ahead" + (if $check.result == "clean" then " · clean"
        elif $check.result == "conflicts" then " · conflicts " + (if ($check.with | length) > 0 then ($check.with | map("#\(.)") | join(" ")) else $check.branch end)
          # The first file in conflict, by its name alone, and how many more.
          + (($check.paths // []) | if length > 0 then " in " + (.[0] | split("/") | last)
              + (if length > 1 then " +\(length - 1)" else "" end) else "" end)
        else "" end)
    elif $landed and (($landed.at | since($now)) // 99999) < 1800 then
      "#\($landed.id) landed \($landed.at | since($now) | minutes)"
    else "" end;

# The `sluice_q` token: every branch's submissions in these documents, and
# the places held because their release failed, a checkout two hosts'
# documents list counted once.
def queue_token: ([.[].branches[].depth] | add // 0) as $n
  | ([.[] | . as $d | held_all($d)[]] | unique_by(.key) | length) as $held
  | [if $n > 0 then plural($n; "submission") else empty end, if $held > 0 then held_words($held) else empty end]
  | join(" · ");

# --- Events ------------------------------------------------------------

# An event's key in its repository's told list: its branch, kind,
# submissions and time. One that names no submission is keyed by the rest.
def event_key($branch): "\($branch)|\(.event)|\(.id // ((.ids // []) | map(tostring) | join(",")))|\(.at)";

def events: .branches[] | .branch as $b | .recent[] | {branch: $b, key: event_key($b), event: .};
