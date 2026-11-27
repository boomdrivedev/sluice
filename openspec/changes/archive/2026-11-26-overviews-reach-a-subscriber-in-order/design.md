## Context

`sluice host` reads a repository's overview after each reading that
ends while a client follows the repository (design D6 of the change
`sluice-host-answers-every-read`). `Scheduler::document` starts a task
for each read. The task asks `Joined` for the overview. When the answer
comes, the task sends the overview into the repository's `watch`
channel.

`Joined` makes one call for each key at a time, and it gives each call
a generation. Thus the calls of one repository's overview never
overlap. Each call reads a state that is as new as the state of the
call before it, or newer. But `Joined::ask` gave only the answer. When
two tasks waited at one time, each one sent its answer when its own
wait ended, in any order. A task that waited for an older call could
send after a task that waited for a newer call.

## Decisions

### D1. An answer carries the moment when its call began

`Joined` keeps the moment when each call began with its answer.
`ask_dated` gives the answer and that moment. `ask` gives only the
answer, as before. A `Moment` is in the order of all the moments that
the process marks. The calls of one key do not overlap. Thus the moments put the
answers of the key in the order of the state that each call read.

### D2. An older overview is not sent after a newer one

`reads::document` gives the overview with the moment of its call. The
scheduler keeps the moment of the overview that it last sent.
`send_document` compares and sends under one lock. An overview whose
read began before that moment is older, and `send_document` does not
send it. It sends an overview of the same call, because that overview
is of the same state.

The document that says that the repository is gone has no read.
`send_document` always sends it, as before.

A slow client gets the newest overview, because the `watch` channel
keeps only the newest value.

### D3. Not a single read at a time

A different fix is to start the next read only when the last read
ended. `Joined` already makes one call at a time and joins the asks
that come during a call. A second queue would add a wait and no
order that the moments do not give.

## Risks / Trade-offs

- `send_document` never drops an overview of a newer state that comes
  late, because its moment is the newest. It drops only an older
  overview, and it sent a newer one before it.
