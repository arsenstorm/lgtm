import { afterEach, describe, expect, it } from "vitest";
import { createFakeDb } from "../../test/fake-db";
import type { ImportedGithubComment } from "../../types/github";
import { setDbForTesting } from "./database";
import { insertImportedComments } from "./imported-comments";

afterEach(() => {
  setDbForTesting(null);
});

const COMMENTS: ImportedGithubComment[] = [
  {
    id: "comment-1",
    pullNumber: 1,
    path: "src/foo.ts",
    body: "new comment",
    diffHunk: "@@ -1,1 +1,1 @@\n+const x = 1;",
    originalLine: 1,
    side: "RIGHT",
    authorLogin: "octocat",
    commentedAt: "2024-01-01T00:00:00.000Z",
  },
  {
    id: "comment-2",
    pullNumber: 1,
    path: "src/bar.ts",
    body: "already imported",
    diffHunk: "@@ -1,1 +1,1 @@\n+const y = 2;",
    originalLine: 2,
    side: "RIGHT",
    authorLogin: "octocat",
    commentedAt: "2024-01-01T00:00:00.000Z",
  },
];

describe("insertImportedComments", () => {
  it("returns only the comments whose INSERT OR IGNORE actually inserted a row", async () => {
    const { db, calls, enqueueExecute } = createFakeDb();
    setDbForTesting(db);
    enqueueExecute({ rowsAffected: 1, lastInsertId: 0 });
    enqueueExecute({ rowsAffected: 0, lastInsertId: 0 });

    const inserted = await insertImportedComments("repo-1", COMMENTS);

    expect(inserted).toEqual([COMMENTS[0]]);
    expect(calls).toHaveLength(2);
    expect(calls[0].sql).toContain(
      "INSERT OR IGNORE INTO imported_github_comments"
    );
  });
});
