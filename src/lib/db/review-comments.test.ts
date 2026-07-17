import { afterEach, describe, expect, it } from "vitest";
import { createFakeDb } from "../../test/fake-db";
import type { DiffAnchor } from "../../types/review";
import { setDbForTesting } from "./database";
import { createComment, listSessionComments } from "./review-comments";

const anchor: DiffAnchor = {
  path: "src/foo.ts",
  side: "new",
  startLine: 10,
  endLine: 12,
  baseRevision: "base-sha",
  headRevision: "head-sha",
  hunkHeader: "@@ -8,4 +8,4 @@",
  selectedCode: "const x = 1;",
  contextBefore: "before",
  contextAfter: "after",
  contextHash: "hash123",
};

afterEach(() => {
  setDbForTesting(null);
});

describe("createComment", () => {
  it("issues an INSERT with params aligned to the anchor columns", async () => {
    const { db, calls } = createFakeDb();
    setDbForTesting(db);

    const comment = await createComment({
      reviewSessionId: "session-1",
      anchor,
      body: "please fix",
      language: "typescript",
    });

    expect(calls).toHaveLength(1);
    expect(calls[0].sql).toContain("INSERT INTO review_comments");

    const params = calls[0].params;
    expect(params[0]).toBe(comment.id);
    expect(params[1]).toBe("session-1");
    expect(params[2]).toBe(anchor.path);
    expect(params[3]).toBe(anchor.side);
    expect(params[4]).toBe(anchor.startLine);
    expect(params[5]).toBe(anchor.endLine);
    expect(params[6]).toBe("please fix");
    expect(params[7]).toBe("typescript");
    expect(params[8]).toBe(anchor.selectedCode);
    expect(params[9]).toBe(anchor.contextBefore);
    expect(params[10]).toBe(anchor.contextAfter);
    expect(params[11]).toBe(anchor.contextHash);
    expect(params[12]).toBe(anchor.hunkHeader);
    expect(params[13]).toBe(anchor.baseRevision);
    expect(params[14]).toBe(anchor.headRevision);
    expect(params[15]).toBe(comment.createdAt);

    expect(comment.status).toBe("draft");
    expect(comment.anchor).toEqual(anchor);
  });
});

describe("listSessionComments", () => {
  it("maps a full snake_case row back into a ReviewComment", async () => {
    const { db, enqueueSelect } = createFakeDb();
    setDbForTesting(db);

    enqueueSelect([
      {
        id: "comment-1",
        review_session_id: "session-1",
        file_path: "src/foo.ts",
        side: "old",
        start_line: 3,
        end_line: 5,
        body: "hello",
        status: "published",
        language: "rust",
        selected_code: "fn main() {}",
        context_before: "before ctx",
        context_after: "after ctx",
        context_hash: "hash-abc",
        hunk_header: "@@ -1,3 +1,3 @@",
        base_revision: "base-1",
        head_revision: "head-1",
        created_at: "2024-01-01T00:00:00.000Z",
        updated_at: "2024-01-02T00:00:00.000Z",
      },
    ]);

    const comments = await listSessionComments("session-1");

    expect(comments).toEqual([
      {
        id: "comment-1",
        reviewSessionId: "session-1",
        anchor: {
          path: "src/foo.ts",
          side: "old",
          startLine: 3,
          endLine: 5,
          baseRevision: "base-1",
          headRevision: "head-1",
          hunkHeader: "@@ -1,3 +1,3 @@",
          selectedCode: "fn main() {}",
          contextBefore: "before ctx",
          contextAfter: "after ctx",
          contextHash: "hash-abc",
        },
        body: "hello",
        language: "rust",
        status: "published",
        createdAt: "2024-01-01T00:00:00.000Z",
        updatedAt: "2024-01-02T00:00:00.000Z",
      },
    ]);
  });
});
