## Context

`LocalFiles` keeps a branch's records as files under the repository's
common git directory. A write holds the branch's lock. It puts its
pending write first, then the files, then the version, and it removes
its pending write last. A read holds no lock, because a round holds the
lock for all of its work. A read reads the version, lists the files,
reads each file, and lays a pending write over what it read.

`Pulls::read_followed` reads a followed pull request from GitHub when
its read is due. It claims the read under the lock of the follows, with
`read_at` set to its time. Then it reads GitHub with no lock, and it
puts what it read into the follows. `kept_across`, which a status calls,
lets go of a request that it shows as merged or closed.

## Decisions

### D1. A read of the records is one version whole

A read reads the version before it lists the files and again after it
reads them. It then reads the pending write. The read is whole where
the version did not move, and the pending write is none, or one of that
version or the next. Then no write ended during the read. Each file that
the read got is of that version, or the pending write lays its own file
over it.

Otherwise a write ended during the read, and the read lists and reads
the files again. A file that the read listed and then did not find is
of such a write, so the read does not fail on it. `read_all_finished`
reads the finished records in the same way.

Writes are short, and a round makes few of them. So a read lists the
files again seldom. A read that finds a write ended at each of 100
tries fails and names the branch.

The other way: a lock that a read holds shared and a write holds only
while it puts its files. Each process of every release must then take
that lock, and a read must write a lock file. The records must stay
readable with no sluice process and nothing written (26, core.11).

### D2. A read of GitHub goes only into the follow that it claimed

After the read of GitHub, `read_followed` writes what it read only
where the follows still hold the request with `read_at` at the time of
its own claim. Where the request is gone, a status or a wait let it go,
and it stays gone. Where another process followed it again or read it
since, that process's write stands. A request that GitHub does not have
leaves the follows, as before.

## Risks

- A read of the records under a long run of writes can fail after 100
  tries. It names the branch, as a read that cannot read the records
  does now.
