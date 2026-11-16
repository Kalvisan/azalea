# Ordinatus patch stack

This fork carries the patches [Ordinatus](https://github.com/Kalvisan/ordinatus) needs on top of upstream
[azalea-rs/azalea](https://github.com/azalea-rs/azalea). Nothing here is submitted upstream. The record of why the
fork exists, the patch list and the rules is Ordinatus ADR-0028.

## Layout

- `ordinatus/main` is an upstream commit followed by a short, linear stack of patch commits. It is never merged
  into; it is rebased.
- The stack's first commit adds `ORDINATUS_BASE`, which holds the upstream commit the stack sits on, and this file.
- Every published state of `ordinatus/main` is tagged `ordinatus-<upstream-short-rev>-<n>`: the first 7 characters
  of the base and a counter that starts at 1 for each base. Ordinatus pins these tags. A published tag is never
  moved, deleted or pushed again; a fix is a new tag.
- `main` and the other upstream branches are plain mirrors, never patched. `ordinatus/wip-<slug>` branches hold a
  patch under test; no tag names them.

## One concern per commit

Each patch is one commit that changes one behaviour, with these trailers:

```
Ordinatus-Patch: <slug>
Gate: <Ordinatus scenario names that prove it>
Upstream-Status: not-submitted
```

`Upstream-Status` is always `not-submitted`: no patch is offered upstream. No drive-by edits, formatting changes
or renames, so each rebase stays a replay of small, independent commits. A reviewing agent reads each patch, and
its gates pass on the Ordinatus release image, before Ordinatus pins a tag that carries it. A patch upstream has
made unnecessary is dropped at the next rebase, and the drop is recorded in ADR-0028.

## Rebasing onto a new upstream

```sh
git config rerere.enabled true
git config rerere.autoupdate true
git fetch upstream
git rebase --onto <new-upstream-rev> <old-base> ordinatus/main
```

Then write the new base into `ORDINATUS_BASE` in the stack's first commit and replay the patches on it:

```sh
first=$(git rev-list --reverse <new-upstream-rev>..ordinatus/main | head -1)
git switch --detach "$first"
git rev-parse <new-upstream-rev> > ORDINATUS_BASE
git commit --amend --no-edit -- ORDINATUS_BASE
git cherry-pick "$first"..ordinatus/main
git branch -f ordinatus/main HEAD && git switch ordinatus/main
git tag -a ordinatus-<new-short-rev>-1 -m "Ordinatus patch stack on <new-short-rev>"
```

With rerere enabled, a conflict resolved once is replayed on later rebases.
