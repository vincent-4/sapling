Tests the :abort merge tool

  $ enable rebase fbamend morestatus
  $ setconfig morestatus.show=True
  $ setconfig rebase.singletransaction=True
  $ setconfig rebase.experimental.inmemory=True
  $ setconfig rebase.experimental.inmemorywarning="rebasing in-memory!"
  $ newrepo

  $ hg debugdrawdag <<'EOS'
  > c
  > |
  > b g
  > |/
  > a
  > EOS
  $ hg up -q g
  $ echo "conflict" > c
  $ hg add -q
  $ hg amend -q
  $ hg up c
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "local change" > b


Confirm it fails when rebasing a change that conflicts:
  $ hg rebase -r tip -d . --tool :abort
  rebasing in-memory!
  rebasing 3:955ac081fc7c "g" (tip)
  merging c
  hit merge conflicts, and --tool :abort passed; exiting.
  $ hg st
  M b
  $ cat b
  local change

Confirm rebase without a merge behaves the same:
  $ hg rebase -r tip -d .~1 --tool :abort
  rebasing in-memory!
  rebasing 3:955ac081fc7c "g" (tip)
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/955ac081fc7c-77e57574-rebase.hg

Confirm the flag fails without IMM:

  $ setconfig rebase.experimental.inmemory=False
  $ hg up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  $ hg rebase -r tip -d . --tool :abort
  rebasing 3:20b9638feb86 "g" (tip)
  merging c
  transaction abort!
  rollback completed
  abort: --tool :abort only works with in-memory merge
  [255]

Confirm that it rebases a three-way merge, but no conflict:
  $ newrepo
  $ $TESTDIR/seq.py 1 5 > a
  $ hg commit -Aq -m "base"
  $ $TESTDIR/seq.py 1 10 > a
  $ hg commit -q -m "extend to 10"
  $ hg up -q .~1
  $ $TESTDIR/seq.py 0 5 > a
  $ hg commit -q -m "prepend with 0"
  $ hg log -G -r 0:: -T '{rev} {desc}'
  @  2 prepend with 0
  |
  | o  1 extend to 10
  |/
  o  0 base
  
  $ hg up -qC 0
  $ hg rebase -r 1 -d 2 --tool :abort
  rebasing in-memory!
  rebasing 1:12cba56c6d27 "extend to 10"
  merging a
  saved backup bundle to $TESTTMP/repo2/.hg/strip-backup/12cba56c6d27-14ff6d99-rebase.hg
  $ hg cat -r tip a | wc -l | xargs
  11

^ (xargs is used for trimming)
