import { afterEach, describe, expect, it } from "vitest";
import { createFakeDb } from "../../test/fake-db";
import type { MemoryFingerprint } from "../../types/review";
import { setDbForTesting } from "./database";
import { createMemoryExample, listEnabledExamples } from "./memory-examples";

afterEach(() => {
  setDbForTesting(null);
});

describe("fingerprint JSON round-trip", () => {
  it("stores the fingerprint as JSON and reads it back through the row mapper", async () => {
    const { db, enqueueSelect } = createFakeDb();
    setDbForTesting(db);

    const fingerprint: MemoryFingerprint = {
      trigrams: ["abc", "bcd"],
      shape: ["ID", "OP"],
      identifiers: ["foo"],
      lineCount: 3,
    };

    const created = await createMemoryExample({
      sourceCommentId: null,
      repositoryId: "repo-1",
      scope: "repository",
      language: "typescript",
      commentBody: "nice catch",
      selectedCode: "const x = 1;",
      contextBefore: "",
      contextAfter: "",
      filePath: "src/foo.ts",
      normalizedCode: "const X = N;",
      fingerprint,
    });

    expect(created.fingerprint).toEqual(fingerprint);

    enqueueSelect([
      {
        id: created.id,
        source_comment_id: null,
        repository_id: "repo-1",
        scope: "repository",
        language: "typescript",
        comment_body: "nice catch",
        selected_code: "const x = 1;",
        context_before: "",
        context_after: "",
        file_path: "src/foo.ts",
        normalized_code: "const X = N;",
        fingerprint: JSON.stringify(fingerprint),
        enabled: 1,
        positive_feedback: 0,
        negative_feedback: 0,
        created_at: created.createdAt,
        updated_at: created.updatedAt,
      },
    ]);

    const [example] = await listEnabledExamples({
      language: "typescript",
      repositoryId: "repo-1",
    });
    expect(example.fingerprint).toEqual(fingerprint);
  });

  it("returns the empty fingerprint for malformed JSON", async () => {
    const { db, enqueueSelect } = createFakeDb();
    setDbForTesting(db);

    enqueueSelect([
      {
        id: "example-1",
        source_comment_id: null,
        repository_id: null,
        scope: "global",
        language: null,
        comment_body: "body",
        selected_code: "code",
        context_before: "",
        context_after: "",
        file_path: "src/foo.ts",
        normalized_code: "code",
        fingerprint: "{not valid json",
        enabled: 1,
        positive_feedback: 0,
        negative_feedback: 0,
        created_at: "2024-01-01T00:00:00.000Z",
        updated_at: "2024-01-01T00:00:00.000Z",
      },
    ]);

    const [example] = await listEnabledExamples({
      language: null,
      repositoryId: null,
    });
    expect(example.fingerprint).toEqual({
      trigrams: [],
      shape: [],
      identifiers: [],
      lineCount: 0,
    });
  });
});

describe("listEnabledExamples", () => {
  it("uses language IS NULL when language is null", async () => {
    const { db, calls, enqueueSelect } = createFakeDb();
    setDbForTesting(db);
    enqueueSelect([]);

    await listEnabledExamples({ language: null, repositoryId: "repo-1" });

    expect(calls[0].sql).toContain("language IS NULL");
    expect(calls[0].sql).not.toContain("language = $");
    expect(calls[0].params).toEqual(["repo-1"]);
  });

  it("uses language = ? when language is provided", async () => {
    const { db, calls, enqueueSelect } = createFakeDb();
    setDbForTesting(db);
    enqueueSelect([]);

    await listEnabledExamples({
      language: "typescript",
      repositoryId: "repo-1",
    });

    expect(calls[0].sql).toContain("language = $1");
    expect(calls[0].sql).not.toContain("language IS NULL");
    expect(calls[0].params).toEqual(["typescript", "repo-1"]);
  });
});
