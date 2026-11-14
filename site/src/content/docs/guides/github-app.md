---
title: Protecting main on GitHub
description: "Make sluice the only way onto main: a GitHub App that only sluice uses is the one exception to main's ruleset, so every other push, your agents' included, is refused."
sidebar:
  order: 5
---

With [main on GitHub](/sluice/guides/github/), sluice pushes each
landing with the credentials your own `git push` uses. Your agents run
git with those same credentials, so GitHub can't tell an agent's
`git push origin HEAD:main` from sluice's landing. This guide gives
sluice an identity of its own: a GitHub App. The app is the only bypass
of `main`'s ruleset, and only sluice's own fetches and pushes carry its
token.

## 1. Create the app

On GitHub, open **Settings › Developer settings › GitHub Apps › New
GitHub App**, in the organization that owns the repository if it has
one. Give it a name, such as `sluice for acme`, and any homepage URL.
Clear **Webhook › Active**: sluice asks GitHub, and GitHub never calls
sluice.

Under **Repository permissions**:

| permission | access |
|---|---|
| Contents | Read and write |
| Workflows | Read and write |
| Commit statuses | Read and write |
| Checks | Read and write |
| Pull requests | Read and write |
| Metadata | Read-only |

sluice needs contents to land, and workflows to land any change under
`.github/workflows`, since GitHub refuses an app's push of one without
it. The statuses, checks and pull requests are for
[pull requests](/sluice/guides/pull-requests/).
Under **Where can this GitHub App be installed?** choose **Only on this
account**, then **Create GitHub App**. Note the **App ID** at the top
of the app's page.

## 2. Install it on the repository

On the app's page, **Install App**, choose the account, then **Only
select repositories** and pick the repository whose `main` sluice
lands on.

## 3. Keep its key on this machine

A host whose own tool keeps the app's key and gives its token skips
this step. Step 4 names that tool's command in place of the key.

On the app's page, **Private keys › Generate a private key** downloads
a `.pem` file. Move it next to sluice's host config, where only you may
read it:

```console
$ mkdir -p ~/.config/sluice
$ mv ~/Downloads/sluice-for-acme.*.private-key.pem ~/.config/sluice/app.pem
$ chmod 600 ~/.config/sluice/app.pem
```

sluice refuses a key that anyone but you may read. The key never goes
in a repository; anyone who holds it can push as sluice.

## 4. Name the app in the host's config

In `~/.config/sluice/config.toml`, or where the host's configuration is
managed when a tool manages it
([who writes it](/sluice/reference/configuration/#who-writes-it)):

```toml
[forge]
app_id = 123456                  # the App ID from step 1
key = "~/.config/sluice/app.pem"
```

A host whose own tool gives the app's token names that command as
`credential`, in place of `key`, and keeps no key of the app's. Write
the command as a list of its program and then each argument, as the
[configuration reference](/sluice/reference/configuration/#forge)
says:

```toml
[forge]
app_id = 123456
credential = ["/opt/forge/bin/token-helper", "--app", "sluice"]
```

From the next round, sluice makes a token from the key for the
repository, or asks the command for one, keeps it in memory, and gives it only to its own `git fetch`
and `git push`. Nothing is written to `.git/config`, so the git your
agents run never finds it. `sluice status` says who pushes:

```console
$ sluice status main
main at 7c829a5 · 0 in the queue · testing 0 at once
pushed to origin as GitHub app 123456
nothing in the queue
```

GitHub sometimes refuses a token in its first seconds, reading the
repository as not found or the push as denied, and takes the same token
a moment later. sluice then tries the fetch or push again on a token it
makes afresh and waits five seconds to use. It pauses longer before each
try, for up to thirty seconds, or five minutes while a submit or
`sluice wait` waits. No single try runs past thirty seconds: sluice
stops a fetch or push GitHub has not answered by then, tries a fetch
again, and reads the branch after a push. A push goes again only when
GitHub refused it before looking at any branch, and expects the branch
where the first push did, so it never moves a branch that moved in
between. When GitHub
still refuses once that time is spent, sluice reports GitHub's own
words, then what each refused token was, in the order it tried them:
its app, repository, installation, scope and age. For a token the
command gave, it names the command and what GitHub says the token
reaches in place of its installation and scope, then its age and its
end. It never shows the token itself. A token the command gave is
tried again on one the command gives afresh, after sluice asks the
command to erase the refused one.

## 5. Make the app the ruleset's only bypass

In the repository's **Settings › Rules › Rulesets**, open the ruleset on
`main` from [GitHub integration](/sluice/guides/github/#set-up-github).
Under **Bypass list**, **Add bypass**, choose the app, and set it to
**Always allow**. Remove every other entry, your own account and any
role included.

From now on a push to `main` from anyone but sluice is refused, yours
too. To land work, submit it.

Do the same for the ruleset on your feature branches' pattern if they live on GitHub
([feature branches on GitHub](/sluice/guides/github/#feature-branches-on-github)). That
ruleset also restricts creations and deletions, since `sluice branch`
makes and deletes them as the app.

## 6. Check it

```console
$ sluice forge check main
ok      [forge] key /home/me/.config/sluice/app.pem reads
ok      app 123456 makes a token for acme/widgets
ok      the app may write acme/widgets's contents
ok      the app may write acme/widgets's workflows
ok      the app may write acme/widgets's checks
ok      the app may write acme/widgets's pull requests
ok      ruleset "main" on main lets only app 123456 through
```

GitHub shows who may bypass a ruleset only to the repository's admins,
so `sluice forge check` reads the ruleset with your own token, from `gh`
or `GH_TOKEN`. It exits `1` and names what is wrong when anything is,
such as a second bypass left on the ruleset.

## GitHub's allowance of calls

GitHub allows the app a number of calls an hour on each installation.
Every read sluice makes carries the version of the answer it last got
at that address, kept on the host, and GitHub answers an unchanged one
without counting it against the allowance.

When a branch's list of open pull requests has changed, or while one of
them waits for a check another system posts, sluice reads them all with
one GraphQL query. It reads their reviews, the checks on their heads and
when they were marked ready for review, in place of reads for each pull
request. That query counts against GitHub's GraphQL allowance, which is
separate from the one its other calls count against. If GitHub refuses
the query for a reason other than a spent allowance, sluice reads each
pull request on its own for that pass and says so once.

When GitHub says the allowance is spent, every sluice on the host stops
calling it until the time GitHub named, or for a minute, then longer,
where it names none. None calls to find out whether the time has come.
While it lasts:

- a `sluice submit` that waits, or a `sluice wait`, waits it out within
  its `--timeout`, saying once on stderr until when, then goes on;
- a `sluice submit --no-wait` fails at once, exit `3`, saying until
  when;
- sluice's host process, and the round `sluice step` asks of it, leave
  the pull requests until the time has passed, say so once, and go on
  landing what has passed its checks.

## What the app does not do

The key is a file your user may read, and an agent runs as your user.
The ruleset keeps agents off `main` because their git never uses the
key, not because they can't read it. A command that gives the token
keeps the key out of your user's files, but an agent may run that
command as it may read a key. Running agents as another user would
close that gap; sluice doesn't do that yet.
