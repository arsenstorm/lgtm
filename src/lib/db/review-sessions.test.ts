import { afterEach, describe, expect, it } from "vitest";
import { createFakeDb } from "../../test/fake-db";
import { setDbForTesting } from "./database";
import { getOrCreateOpenSession } from "./review-sessions";

afterEach(() => {
  setDbForTesting(null);
});

describe("getOrCreateOpenSession", () => {
  it("includes pull_number in the match SELECT and the INSERT for a github-pull-request session", async () => {
    const { db, calls, enqueueSelect } = createFakeDb();
    setDbForTesting(db);
    enqueueSelect([]); // no existing open session
    enqueueSelect([
      {
        id: "session-1",
        repository_id: "repo-1",
        source_kind: "github-pull-request",
        base_revision: null,
        head_revision: null,
        base_sha: "base-sha",
        head_sha: "head-sha",
        pull_number: 42,
        status: "open",
        created_at: "2024-01-01T00:00:00.000Z",
        updated_at: "2024-01-01T00:00:00.000Z",
      },
    ]);

    const session = await getOrCreateOpenSession({
      repositoryId: "repo-1",
      sourceKind: "github-pull-request",
      baseRevision: null,
      headRevision: null,
      pullNumber: 42,
    });

    const selectCall = calls[0];
    expect(selectCall.sql).toContain("pull_number IS $4");
    expect(selectCall.params).toEqual([
      "repo-1",
      "github-pull-request",
      null,
      42,
    ]);

    const insertCall = calls[1];
    expect(insertCall.sql).toContain("INSERT INTO review_sessions");
    expect(insertCall.sql).toContain("pull_number");
    expect(insertCall.params).toContain(42);

    expect(session.pullNumber).toBe(42);
    expect(session.sourceKind).toBe("github-pull-request");
  });

  it("defaults pullNumber to null when omitted", async () => {
    const { db, calls, enqueueSelect } = createFakeDb();
    setDbForTesting(db);
    enqueueSelect([]);
    enqueueSelect([
      {
        id: "session-2",
        repository_id: "repo-1",
        source_kind: "working-tree",
        base_revision: null,
        head_revision: "head-sha",
        base_sha: null,
        head_sha: null,
        pull_number: null,
        status: "open",
        created_at: "2024-01-01T00:00:00.000Z",
        updated_at: "2024-01-01T00:00:00.000Z",
      },
    ]);

    await getOrCreateOpenSession({
      repositoryId: "repo-1",
      sourceKind: "working-tree",
      baseRevision: null,
      headRevision: "head-sha",
    });

    expect(calls[0].params).toEqual(["repo-1", "working-tree", null, null]);
  });
});
