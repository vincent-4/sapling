/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/store/Diff.h"

#include <folly/executors/QueuedImmediateExecutor.h>
#include <folly/test/TestUtils.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>

#include "eden/fs/model/git/TopLevelIgnores.h"
#include "eden/fs/store/DiffContext.h"
#include "eden/fs/store/MemoryLocalStore.h"
#include "eden/fs/store/ObjectStore.h"
#include "eden/fs/store/ScmStatusDiffCallback.h"
#include "eden/fs/testharness/FakeBackingStore.h"
#include "eden/fs/testharness/FakeTreeBuilder.h"
#include "eden/fs/testharness/TestUtil.h"

using namespace facebook::eden;
using namespace std::chrono_literals;
using folly::Future;
using folly::StringPiece;
using std::make_shared;
using ::testing::Pair;
using ::testing::UnorderedElementsAre;

namespace facebook {
namespace eden {
inline void PrintTo(ScmFileStatus status, ::std::ostream* os) {
  switch (status) {
    case ScmFileStatus::ADDED:
      *os << "ADDED";
      return;
    case ScmFileStatus::MODIFIED:
      *os << "MODIFIED";
      return;
    case ScmFileStatus::REMOVED:
      *os << "REMOVED";
      return;
    case ScmFileStatus::IGNORED:
      *os << "IGNORED";
      return;
  }
  *os << "unknown status " << static_cast<unsigned int>(status);
}
} // namespace eden
} // namespace facebook

class DiffTest : public ::testing::Test {
 protected:
  void SetUp() override {
    localStore_ = make_shared<MemoryLocalStore>();
    backingStore_ = make_shared<FakeBackingStore>(localStore_);
    store_ = ObjectStore::create(
        localStore_,
        backingStore_,
        std::make_shared<EdenStats>(),
        &folly::QueuedImmediateExecutor::instance());
  }

  Future<std::unique_ptr<ScmStatus>> diffCommits(
      StringPiece commit1,
      StringPiece commit2) {
    return diffCommitsForStatus(
        store_.get(), makeTestHash(commit1), makeTestHash(commit2));
  }

  ScmStatus diffCommitsWithGitIgnore(
      Hash hash1,
      Hash hash2,
      std::string gitIgnoreContents = {},
      std::string userIgnoreContents = {},
      std::string systemIgnoreContents = {},
      bool listIgnored = true) {
    auto callback = std::make_unique<ScmStatusDiffCallback>();
    auto callbackPtr = callback.get();
    auto mockedLoadFile = [gitIgnoreContents](RelativePathPiece /**/) {
      return folly::makeFuture(gitIgnoreContents);
    };
    auto topLevelIgnores = std::make_unique<TopLevelIgnores>(
        userIgnoreContents, systemIgnoreContents);
    auto gitIgnoreStack = topLevelIgnores->getStack();
    auto diffContext = DiffContext(
        callbackPtr,
        listIgnored,
        store_.get(),
        std::move(topLevelIgnores),
        mockedLoadFile);

    return diffTrees(
               &diffContext,
               RelativePathPiece{},
               hash1,
               hash2,
               gitIgnoreStack,
               false)
        .thenValue([callback = std::move(callback)](auto&&) {
          return callback->extractStatus();
        })
        .get(100ms);
  }

  std::shared_ptr<LocalStore> localStore_;
  std::shared_ptr<FakeBackingStore> backingStore_;
  std::shared_ptr<ObjectStore> store_;
};

TEST_F(DiffTest, unknownCommit) {
  auto future = diffCommits("1", "1");
  EXPECT_THROW_RE(
      std::move(future).get(100ms), std::domain_error, "commit .* not found");
}

TEST_F(DiffTest, sameCommit) {
  FakeTreeBuilder builder;

  builder.setFile("a/b/c/d/e/f.txt", "contents");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto result = diffCommits("1", "1").get(100ms);
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  EXPECT_THAT(result->entries, UnorderedElementsAre());
}

TEST_F(DiffTest, basicDiff) {
  FakeTreeBuilder builder;

  builder.setFile("a/b/c/d/e/f.txt", "contents");
  builder.setFile("a/b/1.txt", "1");
  builder.setFile("a/b/2.txt", "2");
  builder.setFile("a/b/3.txt", "3");
  builder.setFile("src/main.c", "hello world");
  builder.setFile("src/lib.c", "helper code");
  builder.setFile("src/test/test.c", "testing");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  // Modify one file, add one file, and remove one file
  auto builder2 = builder.clone();
  builder2.replaceFile("src/main.c", "hello world v2");
  builder2.setFile("src/test/test2.c", "another test");
  builder2.removeFile("a/b/1.txt");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommits("1", "2").get(100ms);
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result->entries,
      UnorderedElementsAre(
          Pair("src/main.c", ScmFileStatus::MODIFIED),
          Pair("src/test/test2.c", ScmFileStatus::ADDED),
          Pair("a/b/1.txt", ScmFileStatus::REMOVED)));
}

TEST_F(DiffTest, directoryOrdering) {
  FakeTreeBuilder builder;

  // Test adding and removing files at the beginning and end of the sorted
  // directory list.  This exercises different code paths in the diff logic.
  builder.setFile("src/foo/bbb.txt", "b");
  builder.setFile("src/foo/ccc.txt", "c");
  builder.setFile("src/foo/xxx.txt", "x");
  builder.setFile("src/foo/yyy.txt", "y");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();
  builder2.setFile("src/foo/aaa.txt", "a");
  builder2.setFile("src/foo/zzz.txt", "z");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommits("1", "2").get(100ms);
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result->entries,
      UnorderedElementsAre(
          Pair("src/foo/aaa.txt", ScmFileStatus::ADDED),
          Pair("src/foo/zzz.txt", ScmFileStatus::ADDED)));

  auto result2 = diffCommits("2", "1").get(100ms);
  EXPECT_THAT(result2->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result2->entries,
      UnorderedElementsAre(
          Pair("src/foo/aaa.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/zzz.txt", ScmFileStatus::REMOVED)));
}

#ifndef _WIN32
// Not running this test on Windows because of the broken symlink support
TEST_F(DiffTest, modeChange) {
  FakeTreeBuilder builder;

  builder.setFile("some_file", "contents");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  // Modify one file, add one file, and remove one file
  auto builder2 = builder.clone();
  builder2.replaceSymlink("some_file", "contents");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommits("1", "2").get(100ms);
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result->entries,
      UnorderedElementsAre(Pair("some_file", ScmFileStatus::MODIFIED)));

  auto result2 = diffCommits("2", "1").get(100ms);
  EXPECT_THAT(result2->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result2->entries,
      UnorderedElementsAre(Pair("some_file", ScmFileStatus::MODIFIED)));
}
#endif // !_WIN32

TEST_F(DiffTest, newDirectory) {
  FakeTreeBuilder builder;

  builder.setFile("src/foo/a.txt", "a");
  builder.setFile("src/foo/b.txt", "b");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();
  builder2.setFile("src/foo/a/b/c.txt", "c");
  builder2.setFile("src/foo/a/b/d.txt", "d");
  builder2.setFile("src/foo/a/b/e.txt", "e");
  builder2.setFile("src/foo/a/b/f/g.txt", "g");
  builder2.setFile("src/foo/z/y/x.txt", "x");
  builder2.setFile("src/foo/z/y/w.txt", "w");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommits("1", "2").get(100ms);
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result->entries,
      UnorderedElementsAre(
          Pair("src/foo/a/b/c.txt", ScmFileStatus::ADDED),
          Pair("src/foo/a/b/d.txt", ScmFileStatus::ADDED),
          Pair("src/foo/a/b/e.txt", ScmFileStatus::ADDED),
          Pair("src/foo/a/b/f/g.txt", ScmFileStatus::ADDED),
          Pair("src/foo/z/y/x.txt", ScmFileStatus::ADDED),
          Pair("src/foo/z/y/w.txt", ScmFileStatus::ADDED)));

  auto result2 = diffCommits("2", "1").get(100ms);
  EXPECT_THAT(result2->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result2->entries,
      UnorderedElementsAre(
          Pair("src/foo/a/b/c.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/d.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/e.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/f/g.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/z/y/x.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/z/y/w.txt", ScmFileStatus::REMOVED)));
}

TEST_F(DiffTest, fileToDirectory) {
  FakeTreeBuilder builder;

  builder.setFile("src/foo/a.txt", "a");
  builder.setFile("src/foo/b.txt", "b", /* executable */ true);
  builder.setFile("src/foo/a", "regular file");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();
  builder2.removeFile("src/foo/a");
  builder2.setFile("src/foo/a/b/c.txt", "c");
  builder2.setFile("src/foo/a/b/d.txt", "d");
  builder2.setFile("src/foo/a/b/e.txt", "e");
  builder2.setFile("src/foo/a/b/f/g.txt", "g");
  builder2.setFile("src/foo/z/y/x.txt", "x");
  builder2.setFile("src/foo/z/y/w.txt", "w");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommits("1", "2").get(100ms);
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result->entries,
      UnorderedElementsAre(
          Pair("src/foo/a", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/c.txt", ScmFileStatus::ADDED),
          Pair("src/foo/a/b/d.txt", ScmFileStatus::ADDED),
          Pair("src/foo/a/b/e.txt", ScmFileStatus::ADDED),
          Pair("src/foo/a/b/f/g.txt", ScmFileStatus::ADDED),
          Pair("src/foo/z/y/x.txt", ScmFileStatus::ADDED),
          Pair("src/foo/z/y/w.txt", ScmFileStatus::ADDED)));

  auto result2 = diffCommits("2", "1").get(100ms);
  EXPECT_THAT(result2->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result2->entries,
      UnorderedElementsAre(
          Pair("src/foo/a", ScmFileStatus::ADDED),
          Pair("src/foo/a/b/c.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/d.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/e.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a/b/f/g.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/z/y/x.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/z/y/w.txt", ScmFileStatus::REMOVED)));
}

TEST_F(DiffTest, blockedFutures) {
  FakeTreeBuilder builder;

  // Build the commits, but do not make the data ready yet in the
  // FakeBackingStore, so that Futures needing this data will not complete
  // immediately.

  // Create data for the first commit
  builder.setFile("a/b/c/d/e/f.txt", "contents");
  builder.setFile("a/b/1.txt", "1");
  builder.setFile("a/b/2.txt", "2");
  builder.setFile("a/b/3.txt", "3");
  builder.setFile("src/main.c", "hello world");
  builder.setFile("src/lib.c", "helper code");
  builder.setFile("src/test/test.c", "testing");
  builder.finalize(backingStore_, /* setReady */ false);
  auto root1 = backingStore_->putCommit("1", builder);

  // Create data for the second commit
  auto builder2 = builder.clone();
  builder2.replaceFile("src/main.c", "hello world v2");
  builder2.setFile("src/test/test2.c", "another test");
  builder2.removeFile("a/b/c/d/e/f.txt");
  builder2.replaceFile("a/b/1.txt", "1", /* executable */ true);
  builder2.setFile("src/newdir/a.txt", "a");
  builder2.setFile("src/newdir/b/c.txt", "c");
  builder2.setFile("src/newdir/b/d.txt", "d");
  builder2.finalize(backingStore_, /* setReady */ false);
  auto root2 = backingStore_->putCommit("2", builder2);

  auto resultFuture = diffCommits("1", "2");
  EXPECT_FALSE(resultFuture.isReady());

  // Now gradually mark the data in each commit ready, so the diff
  // will make progress as we mark more things ready.

  // Make the root commit & tree for commit 1
  root1->setReady();
  builder.setReady("");
  EXPECT_FALSE(resultFuture.isReady());

  // Mark everything under src/ ready in both trees
  builder.setAllReadyUnderTree("src");
  builder2.setAllReadyUnderTree("src");
  EXPECT_FALSE(resultFuture.isReady());

  // Mark the root commit and tree ready for commit 2.
  root2->setReady();
  builder2.setReady("");
  EXPECT_FALSE(resultFuture.isReady());

  // Mark the hierarchy under "a" ready.
  // Note that we don't have to mark blobs ready, the diffing code
  // only needs to get the tree data.
  builder.setReady("a");
  builder2.setReady("a");
  EXPECT_FALSE(resultFuture.isReady());
  builder.setReady("a/b");
  builder2.setReady("a/b");
  EXPECT_FALSE(resultFuture.isReady());
  builder.setReady("a/b/c");
  EXPECT_FALSE(resultFuture.isReady());
  builder.setReady("a/b/c/d");
  EXPECT_FALSE(resultFuture.isReady());
  // a/b/c/d/e is the last directory that remains not ready yet.
  // Even though we mark it as ready, we still need the files themselves to be
  // ready since we compare blobs in the diff operation
  builder.setReady("a/b/c/d/e");
  EXPECT_FALSE(resultFuture.isReady());

  // Set all files as ready as well.
  builder.setAllReady();
  builder2.setAllReady();
  EXPECT_TRUE(resultFuture.isReady());

  auto result = std::move(resultFuture).get();
  EXPECT_THAT(result->errors, UnorderedElementsAre());
  EXPECT_THAT(
      result->entries,
      UnorderedElementsAre(
          Pair("src/main.c", ScmFileStatus::MODIFIED),
          Pair("src/test/test2.c", ScmFileStatus::ADDED),
          Pair("a/b/c/d/e/f.txt", ScmFileStatus::REMOVED),
          Pair("a/b/1.txt", ScmFileStatus::MODIFIED),
          Pair("src/newdir/a.txt", ScmFileStatus::ADDED),
          Pair("src/newdir/b/c.txt", ScmFileStatus::ADDED),
          Pair("src/newdir/b/d.txt", ScmFileStatus::ADDED)));
}

TEST_F(DiffTest, loadTreeError) {
  FakeTreeBuilder builder;

  // Create data for the first commit
  builder.setFile("a/b/1.txt", "1");
  builder.setFile("a/b/2.txt", "2");
  builder.setFile("a/b/3.txt", "3");
  builder.setFile("x/y/test.txt", "test");
  builder.setFile("x/y/z/file1.txt", "file1");
  builder.finalize(backingStore_, /* setReady */ false);
  auto root1 = backingStore_->putCommit("1", builder);

  // Create data for the second commit
  auto builder2 = builder.clone();
  builder2.replaceFile("a/b/3.txt", "new3");
  builder2.setFile("x/y/z/file2.txt", "file2");
  builder2.finalize(backingStore_, /* setReady */ false);
  auto root2 = backingStore_->putCommit("2", builder2);

  auto resultFuture = diffCommits("1", "2");
  EXPECT_FALSE(resultFuture.isReady());

  // Make the root commit & tree for commit 1
  root1->setReady();
  builder.setReady("");
  root2->setReady();
  builder2.setReady("");
  EXPECT_FALSE(resultFuture.isReady());

  builder.setReady("x");
  builder.setReady("x/y");
  builder.setReady("x/y/z");

  builder2.setReady("x");
  builder2.setReady("x/y");
  // Report an error loading x/y/z on commit2
  builder2.triggerError("x/y/z", std::runtime_error("oh noes"));
  EXPECT_FALSE(resultFuture.isReady());

  builder.setAllReadyUnderTree("a");
  builder2.setAllReadyUnderTree("a");
  // Even though we set everything under a as ready, and the other directories,
  // we still need the all files themselves to be ready since we compare blobs
  // in the diff operation
  EXPECT_FALSE(resultFuture.isReady());

  // Set the rest of files as ready as well.
  builder.setAllReady();
  builder2.setAllReady();
  EXPECT_TRUE(resultFuture.isReady());

  auto result = std::move(resultFuture).get();
  EXPECT_THAT(
      result->errors,
      UnorderedElementsAre(Pair(
          "x/y/z",
          folly::exceptionStr(std::runtime_error("oh noes")).c_str())));
  EXPECT_THAT(
      result->entries,
      UnorderedElementsAre(Pair("a/b/3.txt", ScmFileStatus::MODIFIED)));
}

// Generic test with no ignore files of a an added, modified, and removed file
TEST_F(DiffTest, nonignored_added_modified_and_removed_files) {
  FakeTreeBuilder builder;

  builder.setFile("src/foo/a.txt", "a");
  builder.setFile("src/foo/a", "regular file");
  builder.setFile("src/bar/c", "regular file");
  builder.setFile("src/bar/d.txt", "d", /* executable */ true);
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  // add a file
  auto builder2 = builder.clone();
  builder2.setFile("src/bar/e.txt", "e");
  builder2.removeFile("src/bar/d.txt");
  builder2.replaceFile("src/foo/a.txt", "aa");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(), builder2.getRoot()->get().getHash());
  EXPECT_THAT(result.errors, UnorderedElementsAre());
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          Pair("src/bar/e.txt", ScmFileStatus::ADDED),
          Pair("src/bar/d.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a.txt", ScmFileStatus::MODIFIED)));
}

// Directly test that diffAddedTree marks all files as ADDED in tree (no
// gitignore)
TEST_F(DiffTest, nonignored_added_files) {
  FakeTreeBuilder builder;

  builder.setFile("src/foo/a.txt", "a");
  builder.setFile("src/foo/a", "regular file");
  builder.setFile("src/bar/d.txt", "d", /* executable */ true);
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  // add a subdirectory
  auto builder2 = builder.clone();
  builder2.setFile("src/bar/foo/e.txt", "e");
  builder2.setFile("src/bar/foo/f.txt", "f");

  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(), builder2.getRoot()->get().getHash());
  EXPECT_THAT(result.errors, UnorderedElementsAre());
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          Pair("src/bar/foo/e.txt", ScmFileStatus::ADDED),
          Pair("src/bar/foo/f.txt", ScmFileStatus::ADDED)));

  // Test calling in directly with path to added entries
  auto callback2 = std::make_unique<ScmStatusDiffCallback>();
  auto callbackPtr2 = callback2.get();
  auto diffContext2 = DiffContext(callbackPtr2, store_.get());

  auto result2 = diffAddedTree(
                     &diffContext2,
                     RelativePathPiece{"src/bar/foo"},
                     builder2.getStoredTree(RelativePathPiece{"src/bar/foo"})
                         ->get()
                         .getHash(),
                     nullptr,
                     false)
                     .thenValue([callback = std::move(callback2)](auto&&) {
                       return callback->extractStatus();
                     })
                     .get(100ms);
  EXPECT_THAT(result2.errors, UnorderedElementsAre());
  EXPECT_THAT(
      result2.entries,
      UnorderedElementsAre(
          Pair("src/bar/foo/e.txt", ScmFileStatus::ADDED),
          Pair("src/bar/foo/f.txt", ScmFileStatus::ADDED)));
}

// Directly test that diffRemovedTree marks all files as REMOVED in tree (no
// gitignore)
TEST_F(DiffTest, nonignored_removed_files) {
  FakeTreeBuilder builder;

  builder.setFile("src/foo/b.txt", "b", /* executable */ true);
  builder.setFile("src/bar/c", "regular file");
  builder.setFile("src/bar/foo/e.txt", "e");
  builder.setFile("src/bar/foo/f.txt", "f");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  // remove a subdirectory
  auto builder2 = builder.clone();
  builder2.removeFile("src/bar/foo/e.txt");
  builder2.removeFile("src/bar/foo/f.txt");

  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(), builder2.getRoot()->get().getHash());
  EXPECT_THAT(result.errors, UnorderedElementsAre());
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          Pair("src/bar/foo/e.txt", ScmFileStatus::REMOVED),
          Pair("src/bar/foo/f.txt", ScmFileStatus::REMOVED)));

  // Test calling in directly with path to removed entries
  auto callback2 = std::make_unique<ScmStatusDiffCallback>();
  auto callbackPtr2 = callback2.get();
  auto diffContext2 = DiffContext(callbackPtr2, store_.get());

  auto result2 = diffRemovedTree(
                     &diffContext2,
                     RelativePathPiece{"src/bar/foo"},
                     builder.getStoredTree(RelativePathPiece{"src/bar/foo"})
                         ->get()
                         .getHash())
                     .thenValue([callback = std::move(callback2)](auto&&) {
                       return callback->extractStatus();
                     })
                     .get(100ms);
  EXPECT_THAT(result2.errors, UnorderedElementsAre());
  EXPECT_THAT(
      result2.entries,
      UnorderedElementsAre(
          Pair("src/bar/foo/e.txt", ScmFileStatus::REMOVED),
          Pair("src/bar/foo/f.txt", ScmFileStatus::REMOVED)));
}

// Tests the case in which a tracked file in source control is modified locally.
// In this case, the file should be recorded as MODIFIED, since it matches
// an ignore rule but was already tracked
TEST_F(DiffTest, diff_trees_with_tracked_ignored_file_modified) {
  FakeTreeBuilder builder;

  auto gitIgnoreContents = "a.txt\n";
  builder.setFile("src/foo/a.txt", "a");
  builder.setFile("src/foo/a", "regular file");
  builder.setFile("src/bar/d.txt", "d", /* executable */ true);
  builder.setFile("src/foo/.gitignore", "a.txt\n");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  // add a file
  auto builder2 = builder.clone();
  builder2.setFile("src/bar/e.txt", "e");
  builder2.removeFile("src/bar/d.txt");

  // Even though this is modified, it will be ignored because it matches an
  // ignore rule.
  builder2.replaceFile("src/foo/a.txt", "aa");

  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents);
  EXPECT_THAT(result.errors, UnorderedElementsAre());
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          Pair("src/bar/e.txt", ScmFileStatus::ADDED),
          Pair("src/bar/d.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a.txt", ScmFileStatus::MODIFIED)));
}

// Tests the case in which a tracked file in source control is modified locally.
// In this case, the file should be recorded as MODIFIED, since it matches
// an ignore rule but was already tracked
TEST_F(DiffTest, ignored_added_modified_and_removed_files) {
  FakeTreeBuilder builder;

  auto gitIgnoreContents = "a.txt\n";
  builder.setFile("src/foo/a.txt", "a");
  builder.setFile("src/bar/d.txt", "d", /* executable */ true);
  builder.setFile("src/bar/c", "regular file");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  // add a file
  auto builder2 = builder.clone();
  builder2.setFile("src/foo/.gitignore", gitIgnoreContents);
  builder2.setFile("src/bar/e.txt", "e");
  builder2.removeFile("src/bar/d.txt");
  builder2.replaceFile("src/foo/a.txt", "aa");

  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents);
  EXPECT_THAT(result.errors, UnorderedElementsAre());
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          Pair("src/foo/.gitignore", ScmFileStatus::ADDED),
          Pair("src/bar/e.txt", ScmFileStatus::ADDED),
          Pair("src/bar/d.txt", ScmFileStatus::REMOVED),
          Pair("src/foo/a.txt", ScmFileStatus::MODIFIED)));
}

// Tests that a file that is added that matches a ignore rule is marked as
// IGNORED
TEST_F(DiffTest, ignored_added_files) {
  FakeTreeBuilder builder;

  auto gitIgnoreContents = "foo/e.txt";
  builder.setFile("src/foo/e.txt", "e");
  builder.setFile("src/bar/c.txt", "c");
  builder.setFile("src/bar/.gitignore", gitIgnoreContents);
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  // add a subdirectory
  auto builder2 = builder.clone();
  builder2.setFile("src/bar/foo/e.txt", "e");
  builder2.setFile("src/bar/foo/f.txt", "f");

  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents);
  EXPECT_THAT(result.errors, UnorderedElementsAre());
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          Pair("src/bar/foo/e.txt", ScmFileStatus::IGNORED),
          Pair("src/bar/foo/f.txt", ScmFileStatus::ADDED)));

  auto result2 = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents,
      "",
      "",
      false);
  EXPECT_THAT(result2.errors, UnorderedElementsAre());
  EXPECT_THAT(
      result2.entries,
      UnorderedElementsAre(Pair("src/bar/foo/f.txt", ScmFileStatus::ADDED)));
}

// Test that a file that is tracked by source control but matches an ignore rule
// and is removed is marked as REMOVED since it was previously tracked by source
// control
TEST_F(DiffTest, ignored_removed_files) {
  FakeTreeBuilder builder;

  auto gitIgnoreContents = "foo";
  builder.setFile("src/foo/a.txt", "a");
  builder.setFile("src/bar/c", "regular file");
  builder.setFile("src/bar/foo/e.txt", "e");
  builder.setFile("src/bar/foo/f.txt", "f");
  builder.setFile("src/bar/.gitignore", gitIgnoreContents);
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  // remove a subdirectory
  auto builder2 = builder.clone();
  // Even though this file is ignored, it should still be marked as removed
  // since it was previously tracked by source control.
  builder2.removeFile("src/bar/foo/e.txt");
  builder2.removeFile("src/bar/foo/f.txt");

  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents);
  EXPECT_THAT(result.errors, UnorderedElementsAre());
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          Pair("src/bar/foo/e.txt", ScmFileStatus::REMOVED),
          Pair("src/bar/foo/f.txt", ScmFileStatus::REMOVED)));
}

TEST_F(DiffTest, ignoreToplevelOnly) {
  FakeTreeBuilder builder;
  auto gitIgnoreContents = "/1.txt\nignore.txt\njunk/\n!important.txt\n";
  builder.setFile(".gitignore", gitIgnoreContents);
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();
  // Add some untracked files, some of which match the ignore patterns
  builder2.setFile("1.txt", "new\n");
  builder2.setFile("ignore.txt", "new\n");
  builder2.setFile("src/1.txt", "new\n");
  builder2.setFile("src/foo/ignore.txt", "new\n");
  builder2.mkdir("src/foo/abc");
  builder2.mkdir("src/foo/abc/xyz");
  builder2.setFile("src/foo/abc/xyz/ignore.txt", "new\n");
  builder2.mkdir("junk");
  builder2.setFile("junk/stuff.txt", "new\n");
  // Even though important.txt matches an include rule, the fact that it
  // is inside an excluded directory takes precedence.
  builder2.setFile("junk/important.txt", "new\n");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents);

  EXPECT_THAT(result.errors, UnorderedElementsAre());
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::ADDED),
          std::make_pair("1.txt", ScmFileStatus::IGNORED),
          std::make_pair("ignore.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/stuff.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/important.txt", ScmFileStatus::IGNORED),
          std::make_pair("src/foo/ignore.txt", ScmFileStatus::IGNORED),
          std::make_pair(
              "src/foo/abc/xyz/ignore.txt", ScmFileStatus::IGNORED)));
}

// Test with a file that matches a .gitignore pattern but also is already in the
// Tree (so we should report the modification)
TEST_F(DiffTest, ignored_file_local_and_in_tree) {
  FakeTreeBuilder builder;

  auto gitIgnoreContents = "/1.txt\nignore.txt\njunk/\n!important.txt\nxyz\n";
  builder.setFile(".gitignore", gitIgnoreContents);
  builder.setFile("src/foo/abc/xyz/ignore.txt", "test\n");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();
  // Add some untracked files, some of which match the ignore patterns
  builder2.setFile("1.txt", "new\n");
  builder2.setFile("ignore.txt", "new\n");
  builder2.setFile("src/1.txt", "new\n");
  builder2.setFile("src/foo/ignore.txt", "new\n");
  builder2.mkdir("junk");
  builder2.setFile("junk/stuff.txt", "new\n");

  // overwrite a file that already exists and matches the ignore pattern
  builder2.replaceFile("src/foo/abc/xyz/ignore.txt", "modified\n");

  // Even though important.txt matches an include rule, the fact that it
  // is inside an excluded directory takes precedence.
  builder2.setFile("junk/important.txt", "new\n");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents);
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::ADDED),
          std::make_pair("src/foo/abc/xyz/ignore.txt", ScmFileStatus::MODIFIED),
          std::make_pair("1.txt", ScmFileStatus::IGNORED),
          std::make_pair("ignore.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/stuff.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/important.txt", ScmFileStatus::IGNORED),
          std::make_pair("src/foo/ignore.txt", ScmFileStatus::IGNORED)));
}

// Test with a file that matches a .gitignore pattern but also is already in the
// Tree but removed from mount (so we should report the file removal)
TEST_F(DiffTest, ignored_file_not_local_but_is_in_tree) {
  FakeTreeBuilder builder;

  auto gitIgnoreContents = "/1.txt\nignore.txt\njunk/\n!important.txt\nxyz\n";
  builder.setFile(".gitignore", gitIgnoreContents);
  builder.setFile("src/foo/abc/xyz/ignore.txt", "test\n");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();

  // Add some untracked files, some of which match the ignore patterns
  builder2.setFile("1.txt", "new\n");
  builder2.setFile("ignore.txt", "new\n");
  builder2.setFile("src/1.txt", "new\n");
  builder2.setFile("src/foo/ignore.txt", "new\n");
  builder2.mkdir("junk");
  builder2.setFile("junk/stuff.txt", "new\n");

  // remove a file that already exists and matches the ignore pattern
  builder2.removeFile("src/foo/abc/xyz/ignore.txt");

  // Even though important.txt matches an include rule, the fact that it
  // is inside an excluded directory takes precedence.
  builder2.setFile("junk/important.txt", "new\n");

  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents);
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          std::make_pair("src/1.txt", ScmFileStatus::ADDED),
          std::make_pair("src/foo/abc/xyz/ignore.txt", ScmFileStatus::REMOVED),
          std::make_pair("1.txt", ScmFileStatus::IGNORED),
          std::make_pair("ignore.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/stuff.txt", ScmFileStatus::IGNORED),
          std::make_pair("junk/important.txt", ScmFileStatus::IGNORED),
          std::make_pair("src/foo/ignore.txt", ScmFileStatus::IGNORED)));
}

// Test with a .gitignore file in the top-level directory
// and the presence of both of system level and user specific ignore files
TEST_F(DiffTest, ignoreSystemLevelAndUser) {
  FakeTreeBuilder builder;

  auto gitIgnoreContents = "/1.txt\nignore.txt\njunk/\n!important.txt\n";
  builder.setFile(".gitignore", gitIgnoreContents);
  builder.setFile("src/foo/bar.txt", "test\n");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();

  // Add some untracked files, matching either global or user patterns
  builder2.setFile("skip_global.txt", "new\n");
  builder2.setFile("skip_user.txt", "new\n");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents,
      "skip_global.txt\n",
      "skip_user.txt\n");
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          std::make_pair("skip_global.txt", ScmFileStatus::IGNORED),
          std::make_pair("skip_user.txt", ScmFileStatus::IGNORED)));
}

// Test with a .gitignore file in the top-level directory
// and the presence of user specific ignore file
TEST_F(DiffTest, ignoreUserLevel) {
  FakeTreeBuilder builder;

  auto gitIgnoreContents = "/1.txt\nignore.txt\njunk/\n!important.txt\n";
  builder.setFile(".gitignore", gitIgnoreContents);
  builder.setFile("src/foo/bar.txt", "test\n");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();

  // Add some untracked files, matching either global or user patterns
  builder2.setFile("skip_global.txt", "new\n");
  builder2.setFile("skip_user.txt", "new\n");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents,
      "",
      "skip_user.txt\n");
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          std::make_pair("skip_global.txt", ScmFileStatus::ADDED),
          std::make_pair("skip_user.txt", ScmFileStatus::IGNORED)));
}

// Test with a .gitignore file in the top-level directory
// and the presence of system level ignore file
TEST_F(DiffTest, ignoreSystemLevel) {
  FakeTreeBuilder builder;

  auto gitIgnoreContents = "/1.txt\nignore.txt\njunk/\n!important.txt\n";
  builder.setFile(".gitignore", gitIgnoreContents);
  builder.setFile("src/foo/bar.txt", "test\n");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();

  // Add some untracked files, matching either global or user patterns
  builder2.setFile("skip_global.txt", "new\n");
  builder2.setFile("skip_user.txt", "new\n");
  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents,
      "skip_global.txt\n",
      "");
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          std::make_pair("skip_global.txt", ScmFileStatus::IGNORED),
          std::make_pair("skip_user.txt", ScmFileStatus::ADDED)));
}

// Tests the case in which a tracked directory in source control is replaced by
// a file locally, and the directory matches an ignore rule. In this case,
// the file should be recorded as ADDED, since the ignore rule is specifically
// for directories
TEST_F(DiffTest, directory_to_file_with_directory_ignored) {
  FakeTreeBuilder builder;

  auto gitIgnoreContents = "a/b/";
  builder.setFile("a/b.txt", "test\n");
  builder.setFile("a/b/c.txt", "test\n");
  builder.setFile("a/b/d.txt", "test\n");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();

  builder2.removeFile("a/b/c.txt");
  builder2.removeFile("a/b/d.txt");
  builder2.setFile("a/b", "regular file");
  builder2.setFile(".gitignore", gitIgnoreContents);

  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents);
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          std::make_pair("a/b/c.txt", ScmFileStatus::REMOVED),
          std::make_pair("a/b/d.txt", ScmFileStatus::REMOVED),
          std::make_pair("a/b", ScmFileStatus::ADDED),
          std::make_pair(".gitignore", ScmFileStatus::ADDED)));
}

// Tests the case in which a tracked directory in source control is replaced by
// a file locally, and the file matches an ignore rule. In this case, the file
// should be recorded as IGNORED, since the ignore rule is specifically for
// files
TEST_F(DiffTest, directory_to_file_with_file_ignored) {
  FakeTreeBuilder builder;

  auto gitIgnoreContents = "a/b";
  builder.setFile("a/b.txt", "test\n");
  builder.setFile("a/b/c.txt", "test\n");
  builder.setFile("a/b/d.txt", "test\n");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();

  builder2.removeFile("a/b/c.txt");
  builder2.removeFile("a/b/d.txt");
  builder2.setFile("a/b", "regular file");
  builder2.setFile(".gitignore", gitIgnoreContents);

  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents);
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          std::make_pair("a/b/c.txt", ScmFileStatus::REMOVED),
          std::make_pair("a/b/d.txt", ScmFileStatus::REMOVED),
          std::make_pair("a/b", ScmFileStatus::IGNORED),
          std::make_pair(".gitignore", ScmFileStatus::ADDED)));
}

// Tests the case in which a tracked file in source control is replaced by
// a directory locally, and the file matches an ignore rule. In this case,
// the directory should be recorded as ADDED, since the ignore rule is
// specifically for files
TEST_F(DiffTest, file_to_directory_with_gitignore) {
  FakeTreeBuilder builder;

  auto gitIgnoreContents = "a/b/d\n!a/b/d/";
  builder.setFile("a/b.txt", "test\n");
  builder.setFile("a/b/c.txt", "test\n");
  builder.setFile("a/b/d", "test\n");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();

  builder2.removeFile("a/b/d");
  builder2.mkdir("a/b/d");
  builder2.setFile("a/b/d/e.txt", "test");
  builder2.setFile(".gitignore", gitIgnoreContents);

  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents);
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          std::make_pair("a/b/d", ScmFileStatus::REMOVED),
          std::make_pair("a/b/d/e.txt", ScmFileStatus::ADDED),
          std::make_pair(".gitignore", ScmFileStatus::ADDED)));
}

// Tests the case in which a file is replaced by a directory, and a directory
// is ignored, but a file inside the directory is not ignored.
TEST_F(DiffTest, addIgnoredDirectory) {
  FakeTreeBuilder builder;

  builder.setFile("a/b.txt", "test\n");
  builder.setFile("a/b/c.txt", "test\n");
  builder.setFile("a/b/r", "test\n");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();

  // The following won't be tracked
  builder2.removeFile("a/b/r");
  builder2.mkdir("a/b/r");
  builder2.setFile("a/b/r/e.txt", "ignored");
  builder2.mkdir("a/b/r/d");
  builder2.setFile("a/b/r/d/g.txt", "ignored too");

  // The following should be tracked
  builder2.mkdir("a/b/g");
  builder2.setFile("a/b/g/e.txt", "added");

  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  // It is not possible to re-include a file if a parent directory of that file
  // is excluded.
  auto systemIgnore = "a/b/r/\n!a/b/r/d/g.txt\n";
  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      "",
      systemIgnore);

  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          std::make_pair("a/b/r", ScmFileStatus::REMOVED),
          std::make_pair("a/b/r/e.txt", ScmFileStatus::IGNORED),
          std::make_pair("a/b/r/d/g.txt", ScmFileStatus::IGNORED),
          std::make_pair("a/b/g/e.txt", ScmFileStatus::ADDED)));
}

// Tests the case in which a file becomes a directory and the directory is
// ignored but the parent directory is not ignored.
TEST_F(DiffTest, nestedGitIgnoreFiles) {
  FakeTreeBuilder builder;

  // a/b/r/e.txt is not ignored.
  builder.setFile("a/b.txt", "test\n");
  builder.setFile("a/b/c.txt", "test\n");
  builder.setFile("a/b/r", "test\n");
  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();

  auto gitIgnoreContents = "!e.txt\n";
  builder2.removeFile("a/b/r");
  builder2.mkdir("a/b/r");
  builder2.setFile("a/b/r/e.txt", "not ignored");
  builder2.setFile("a/b/r/f.txt", "is ignored");
  builder2.setFile("a/b/r/.gitignore", gitIgnoreContents);

  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto systemIgnore = "a/b/r/*\n!a/b/r/.gitignore\n";
  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      gitIgnoreContents,
      systemIgnore);
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(
          std::make_pair("a/b/r", ScmFileStatus::REMOVED),
          std::make_pair("a/b/r/e.txt", ScmFileStatus::ADDED),
          std::make_pair("a/b/r/f.txt", ScmFileStatus::IGNORED),
          std::make_pair("a/b/r/.gitignore", ScmFileStatus::ADDED)));
}

// Tests the case in which hidden folders (like .hg/.eden) are not reported
TEST_F(DiffTest, hiddenFolder) {
  FakeTreeBuilder builder;

  builder.setFile("a/b.txt", "test\n");

  builder.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("1", builder)->setReady();

  auto builder2 = builder.clone();

  builder2.setFile("a/c.txt", "not ignored");

  // There should be no mention of this in the results.
  builder2.mkdir(".hg");

  builder2.finalize(backingStore_, /* setReady */ true);
  backingStore_->putCommit("2", builder2)->setReady();

  auto result = diffCommitsWithGitIgnore(
      builder.getRoot()->get().getHash(),
      builder2.getRoot()->get().getHash(),
      "");
  EXPECT_THAT(
      result.entries,
      UnorderedElementsAre(std::make_pair("a/c.txt", ScmFileStatus::ADDED)));
}
