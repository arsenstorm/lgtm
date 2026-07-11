import { afterEach, describe, expect, it } from "vitest";
import { createFakeDb } from "../../test/fake-db";
import type { ImportedGithubComment } from "../../types/github";
import { setDbForTesting } from "../db/database";
import {
  deriveExamplesFromImports,
  diffHunkToSelectedCode,
} from "./import-memory";

afterEach(() => {
  setDbForTesting(null);
});

const BASE_COMMENT: ImportedGithubComment = {
  id: "comment-1",
  pullNumber: 1,
  path: "src/foo.ts",
  body: "",
  diffHunk: "",
  originalLine: 10,
  side: "RIGHT",
  authorLogin: "octocat",
  commentedAt: "2024-01-01T00:00:00.000Z",
};

describe("diffHunkToSelectedCode", () => {
  it("prefers added lines when the hunk has additions", () => {
    const hunk = [
      "@@ -1,3 +1,4 @@",
      " function foo() {",
      "-  return 1;",
      "+  const result = computeValue(a, b, c);",
      "+  return result;",
      " }",
    ].join("\n");

    expect(diffHunkToSelectedCode(hunk)).toBe(
      "  const result = computeValue(a, b, c);\n  return result;"
    );
  });

  it("falls back to context and deleted lines for deletion-only hunks", () => {
    const hunk = [
      "@@ -1,3 +1,2 @@",
      " function foo() {",
      "-  return 1;",
      " }",
    ].join("\n");

    expect(diffHunkToSelectedCode(hunk)).toBe(
      "function foo() {\n  return 1;\n}"
    );
  });
});

describe("deriveExamplesFromImports", () => {
  it("creates a memory example for a substantive comment on real code", async () => {
    const { db, calls } = createFakeDb();
    setDbForTesting(db);

    const comment: ImportedGithubComment = {
      ...BASE_COMMENT,
      body: "Please extract this into a helper function for reuse.",
      diffHunk: [
        "@@ -1,3 +1,4 @@",
        " function foo() {",
        "-  return 1;",
        "+  const result = computeValue(a, b, c);",
        "+  return result;",
        " }",
      ].join("\n"),
    };

    const created = await deriveExamplesFromImports("repo-1", [comment]);

    expect(created).toBe(1);
    const insertCalls = calls.filter((c) =>
      c.sql.includes("INSERT INTO memory_examples")
    );
    expect(insertCalls).toHaveLength(1);
  });

  it("skips generic or short comment bodies", async () => {
    const { db, calls } = createFakeDb();
    setDbForTesting(db);

    const comment: ImportedGithubComment = {
      ...BASE_COMMENT,
      body: "lgtm",
      diffHunk: [
        "@@ -1,3 +1,4 @@",
        " function foo() {",
        "-  return 1;",
        "+  const result = computeValue(a, b, c);",
        "+  return result;",
        " }",
      ].join("\n"),
    };

    const created = await deriveExamplesFromImports("repo-1", [comment]);

    expect(created).toBe(0);
    expect(
      calls.filter((c) => c.sql.includes("INSERT INTO memory_examples"))
    ).toHaveLength(0);
  });

  it("skips non-code paths that don't resolve to a language", async () => {
    const { db, calls } = createFakeDb();
    setDbForTesting(db);

    const comment: ImportedGithubComment = {
      ...BASE_COMMENT,
      path: "Dockerfile",
      body: "Please extract this into a helper function for reuse.",
      diffHunk: [
        "@@ -1,3 +1,4 @@",
        " FROM node",
        "-RUN foo",
        "+RUN computeValue(a, b, c)",
        "+RUN result",
        " EXPOSE 80",
      ].join("\n"),
    };

    const created = await deriveExamplesFromImports("repo-1", [comment]);

    expect(created).toBe(0);
    expect(
      calls.filter((c) => c.sql.includes("INSERT INTO memory_examples"))
    ).toHaveLength(0);
  });
});
