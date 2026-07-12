import { afterEach, describe, expect, it } from "vitest";
import { createFakeDb } from "../../test/fake-db";
import { setDbForTesting } from "./database";
import { mergeRepositoryRecords } from "./repositories";

afterEach(() => {
  setDbForTesting(null);
});

describe("mergeRepositoryRecords", () => {
  it("reassigns review_sessions, memory_examples, and imported_github_comments then deletes the source repository, in order", async () => {
    const { db, calls } = createFakeDb();
    setDbForTesting(db);

    await mergeRepositoryRecords("from-id", "to-id");

    expect(calls).toHaveLength(4);

    expect(calls[0].sql).toContain("UPDATE review_sessions");
    expect(calls[0].sql).toContain("repository_id = $1");
    expect(calls[0].sql).toContain("repository_id = $2");
    expect(calls[0].params).toEqual(["to-id", "from-id"]);

    expect(calls[1].sql).toContain("UPDATE memory_examples");
    expect(calls[1].params).toEqual(["to-id", "from-id"]);

    expect(calls[2].sql).toContain("UPDATE imported_github_comments");
    expect(calls[2].params).toEqual(["to-id", "from-id"]);

    expect(calls[3].sql).toContain("DELETE FROM repositories");
    expect(calls[3].params).toEqual(["from-id"]);
  });

  it("is a no-op when fromId and toId are the same", async () => {
    const { db, calls } = createFakeDb();
    setDbForTesting(db);

    await mergeRepositoryRecords("same-id", "same-id");

    expect(calls).toHaveLength(0);
  });
});
