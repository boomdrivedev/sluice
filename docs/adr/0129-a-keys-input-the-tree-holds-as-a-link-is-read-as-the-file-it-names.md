---
number: 129
title: A key's input the tree holds as a link is read as the file it names
status: accepted
date: 2026-11-19
decision-makers:
- the design agent, by its ruling of 2026-11-19 for the fix main--the-key-check-reads-a-symlink-as-moon-does
- the code stage of fix/main--the-key-check-reads-a-symlink-as-moon-does
---

# A key's input the tree holds as a link is read as the file it names

Status: accepted. Points 1 and 2 are the design agent's ruling, relayed to the code stage by boomdrive-ops on 2026-11-19. Point 3, how the tree is read, is the code stage's own, made on the rules written and cheap to change.

Deciders: the design agent's ruling: "check a tracked symlink's input against the file it names in the same tree, resolved from the link's own folder, as moon reads it on disk. A link that leaves the tree, or names a path the tree doesn't hold, stays refused. Nothing else in the key check changes." The code stage of `fix/main--the-key-check-reads-a-symlink-as-moon-does`, on point 3.

Sources: the statement's C.2 moon row, where a key is proven by moon's own manifest of it, and C.6; the change `peers`, design D9, which made the key check; rules core.9 and core.22; `crates/sluice/src/profiles/moon.rs` `file_at` and `check_key`; git's `cat-file --batch --follow-symlinks`. The case: flume's `bolt/flume-is-tested-against-the-claude-code-hosts-run`, submission 1, 2026-11-19, on sluice 0.97.4. Its `root:format` failed for nothing the submission changed: `.claude/rules/core-details.md is not what the tree holds`.

## Context and problem statement

A signer signs a key only where the combination's tree bears it out. Every input moon's manifest of the key names must hold, in the tree, the content whose sha256 the manifest gives it (`peers` D9; core.22). sluice read each input with `git cat-file blob <revision>:<path>`. For a file the tree holds as a link, that is the link's own text.

moon 2.6.1 hashes such an input as it reads it on disk, through the link: by the content of the file it names. flume's `.claude/rules/core-details.md` is a link to `../../docs/architecture/core-details.md`. moon's manifest for `root:format` gave it the sha256 of the file it names, `c8678588…`. sluice read the link's text, `3c5de9db…`, and refused the key. Every submission whose task inputs take in such a link failed, on flume's bolt and its main. boomdrive-cloud holds nine such links under `.claude/rules/` and flume one.

## Considered options

For what a link among a key's inputs is checked against:

1. **The file it names in the same tree, resolved from the link's own folder (chosen, the design agent's).**
2. The file it names on the run's disk, wherever it is.
3. The link's own text, as before.

For how the tree is read through a link:

1. **git's `cat-file --batch --follow-symlinks`, one object asked per path (chosen).**
2. `git ls-tree` for the path's mode, then the link's text resolved by sluice's own code and read again.

## Decision outcome

1. **A link among a key's inputs is checked against the file it names in the same tree (the design agent's).** The path is resolved from the link's own folder, as moon reads it on disk. A link that names a link is followed on. Nothing else in the key check changes.
   - Not taken: option 2. A file outside the combination's tree is nothing the combination's tree bears out, and differs from host to host.
   - Not taken: option 3. moon never hashes a link's text, so a key naming a link could never be borne out.
2. **A link the tree cannot follow to a file of its own stays refused (the design agent's).** That is a link that leaves the tree, by an absolute or a relative path, one that names a path the tree does not hold, a loop, or one that names a folder. A key naming it is refused as one naming an input not in the tree: `<task>: <path> is not in the tree`.
3. **git follows the link within the revision's tree (option 1, the code stage's).** `file_at` asks `git cat-file --batch --follow-symlinks -Z` for `<revision>:<path>`, and takes the content only where git answers with a blob. git resolves the link from its own folder, follows a link within the path, and answers `symlink`, `dangling`, `loop`, `notdir` or `missing` for what it cannot follow within the tree, which `file_at` reads as no file. `-Z` keeps a path with a newline whole, as `cat-file blob` did.
   - Not taken: option 2. It is two calls where one does, and resolution of sluice's own of what git already resolves.

### Consequences

- Good: a key whose inputs take in a link inside the tree is borne out, on every host, the sign job included. Flume's and boomdrive-cloud's `.claude/rules/` links no longer fail a submission that changed nothing.
- Good: a link that leaves the tree still refuses its key, so nothing outside the combination stands for its tree.
- Neutral: `cat-file --batch -Z` needs git 2.41 or later. sluice already needs git 2.38 for `merge-tree --write-tree`, and its guides assume 2.48.
- Bad: a key whose task really reads a link out of the tree, as moon would, can never be borne out. The ruling keeps that refused.
