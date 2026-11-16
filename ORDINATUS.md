# Ordinatus patch stack

This fork carries the patches [Ordinatus](https://github.com/Kalvisan/ordinatus) needs on top of upstream
[azalea-rs/azalea](https://github.com/azalea-rs/azalea). Nothing here is submitted upstream. The record of why the
fork exists, the patch list and the rules is Ordinatus ADR-0028.

## Layout

- `ordinatus/main` is an upstream commit followed by a short, linear stack of patch commits. It is never merged
  into; it is rebased.
- `ORDINATUS_BASE` holds the upstream commit the stack sits on. It is the first commit of the stack.
- Every published state is tagged `ordinatus-<upstream-short-rev>-<n>`: the first 7 characters of the base and a
  counter that starts at 1 for each base. Ordinatus pins these tags.

## One concern per commit

Each patch is one commit that changes one behaviour, with these trailers:

```
Ordinatus-Patch: <slug>
Gate: <Ordinatus scenario names that prove it>
Upstream-Status: not-submitted
```

No drive-by edits, formatting changes or renames, so each rebase stays a replay of small, independent commits.
A patch upstream has made unnecessary is dropped at the next rebase, and the drop is recorded in ADR-0028.

## Rebasing onto a new upstream

```sh
git config rerere.enabled true
git config rerere.autoupdate true
git fetch upstream
git rebase --onto <new-upstream-rev> <old-base> ordinatus/main
```

Then write the new base into `ORDINATUS_BASE` by amending the stack's first commit, and tag the result
`ordinatus-<new-short-rev>-1`. With rerere enabled, a conflict resolved once is replayed on later rebases.
