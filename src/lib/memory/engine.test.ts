import { parsePatchFiles } from "@pierre/diffs";
import { describe, expect, it } from "vitest";
import type { MemoryExample } from "../../types/review";
import { generateSuggestions, shouldCreateMemoryExample } from "./engine";
import { lexicalNormalizer } from "./normalize";
import {
  adjustedConfidence,
  buildFingerprint,
  MAX_SUGGESTIONS_PER_FILE,
  SUGGESTION_THRESHOLD,
} from "./similarity";

const REPOSITORY_ID = "repo-1";
const BASE_REVISION = "base-sha";
const HEAD_REVISION = "head-sha";

let exampleCounter = 0;

function makeExample(overrides: Partial<MemoryExample> = {}): MemoryExample {
  const selectedCode = overrides.selectedCode ?? "const value = 1;";
  const normalized = lexicalNormalizer.normalize(selectedCode);
  exampleCounter += 1;
  return {
    id: `example-${exampleCounter}`,
    sourceCommentId: null,
    repositoryId: REPOSITORY_ID,
    scope: "repository",
    language: "typescript",
    commentBody:
      "Use a Set here for O(1) membership checks instead of scanning the array.",
    selectedCode,
    contextBefore: "",
    contextAfter: "",
    filePath: "src/example.ts",
    normalizedCode: normalized.tokens.join(" "),
    fingerprint: buildFingerprint(normalized),
    enabled: true,
    positiveFeedback: 0,
    negativeFeedback: 0,
    createdAt: "2024-01-01T00:00:00.000Z",
    updatedAt: "2024-01-01T00:00:00.000Z",
    ...overrides,
  };
}

function parseFile(patch: string) {
  const parsed = parsePatchFiles(patch);
  const file = parsed[0]?.files[0];
  if (!file) {
    throw new Error("fixture patch failed to parse");
  }
  return file;
}

function callGenerate(
  files: ReturnType<typeof parseFile>[],
  examples: MemoryExample[],
  overrides: Partial<Parameters<typeof generateSuggestions>[0]> = {}
) {
  return generateSuggestions({
    files,
    examples,
    repositoryId: REPOSITORY_ID,
    currentSessionCommentIds: new Set(),
    alreadySuggestedExampleIds: new Set(),
    baseRevision: BASE_REVISION,
    headRevision: HEAD_REVISION,
    ...overrides,
  });
}

const RENAME_EXAMPLE_CODE = [
  "const items = await fetchUsers();",
  "for (const user of items) {",
  "  console.log(user.name);",
  "}",
].join("\n");

const RENAME_PATCH = `diff --git a/src/target.ts b/src/target.ts
index 1111111..2222222 100644
--- a/src/target.ts
+++ b/src/target.ts
@@ -1,2 +1,6 @@
 const start = true;
+const records = await fetchUsers();
+for (const record of records) {
+  console.log(record.name);
+}
 const end = true;
`;

const LITERAL_PATCH = `diff --git a/src/literals.ts b/src/literals.ts
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/src/literals.ts
@@ -0,0 +1,2 @@
+const timeout = 3000;
+const label = "saving";
`;

const DIFFERENT_PATCH = `diff --git a/src/different.ts b/src/different.ts
new file mode 100644
index 0000000..2222222
--- /dev/null
+++ b/src/different.ts
@@ -0,0 +1,4 @@
+function computeChecksum(buffer) {
+  let sum = 0;
+  return sum;
+}
`;

const LANGUAGE_MISMATCH_PATCH = `diff --git a/src/target.py b/src/target.py
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/src/target.py
@@ -0,0 +1,4 @@
+const records = await fetchUsers();
+for (const record of records) {
+  console.log(record.name);
+}
`;

const IMPORT_PATCH = `diff --git a/src/imports.ts b/src/imports.ts
new file mode 100644
index 0000000..4444444
--- /dev/null
+++ b/src/imports.ts
@@ -0,0 +1,3 @@
+import { a } from "./a";
+import { b } from "./b";
+import { c } from "./c";
`;

const CAPS_PATCH = `diff --git a/src/caps.ts b/src/caps.ts
index 1111111..2222222 100644
--- a/src/caps.ts
+++ b/src/caps.ts
@@ -1,5 +1,17 @@
 const header = true;
+if (a === null) {
+  logger.warnA();
+}
 const sep1 = 1;
+if (b === null) {
+  logger.warnB();
+}
 const sep2 = 2;
+if (c === null) {
+  logger.warnC();
+}
 const sep3 = 3;
+if (d === null) {
+  logger.warnD();
+}
 const footer = true;
`;

describe("generateSuggestions", () => {
  it("matches candidate code with renamed variables", () => {
    const file = parseFile(RENAME_PATCH);
    const example = makeExample({
      selectedCode: RENAME_EXAMPLE_CODE,
      contextBefore: "const start = true;",
      contextAfter: "const end = true;",
    });
    const drafts = callGenerate([file], [example]);
    expect(drafts).toHaveLength(1);
    expect(drafts[0].memoryExampleId).toBe(example.id);
    expect(drafts[0].similarityScore).toBeGreaterThanOrEqual(
      SUGGESTION_THRESHOLD
    );
  });

  it("matches candidate code that only changed literals", () => {
    const file = parseFile(LITERAL_PATCH);
    const example = makeExample({
      selectedCode: 'const timeout = 5000;\nconst label = "loading";',
    });
    const drafts = callGenerate([file], [example]);
    expect(drafts).toHaveLength(1);
  });

  it("does not match materially different control flow", () => {
    const file = parseFile(DIFFERENT_PATCH);
    const example = makeExample({ selectedCode: RENAME_EXAMPLE_CODE });
    const drafts = callGenerate([file], [example]);
    expect(drafts).toHaveLength(0);
  });

  it("never matches across languages, even with identical code text", () => {
    const file = parseFile(LANGUAGE_MISMATCH_PATCH);
    const example = makeExample({
      selectedCode: RENAME_EXAMPLE_CODE,
      language: "typescript",
    });
    const drafts = callGenerate([file], [example]);
    expect(drafts).toHaveLength(0);
  });

  it("keeps only the higher-confidence draft when two examples match the same run", () => {
    const file = parseFile(RENAME_PATCH);
    const strongerExample = makeExample({
      selectedCode: RENAME_EXAMPLE_CODE,
      contextBefore: "const start = true;",
      contextAfter: "const end = true;",
      commentBody: "Prefer a for-of loop with a descriptive name.",
    });
    const weakerExample = makeExample({
      selectedCode: RENAME_EXAMPLE_CODE,
      contextBefore: "const start = true;",
      contextAfter: "const end = true;",
      commentBody: "Consider renaming this variable.",
      negativeFeedback: 1,
    });
    const drafts = callGenerate([file], [strongerExample, weakerExample]);
    expect(drafts).toHaveLength(1);
    expect(drafts[0].memoryExampleId).toBe(strongerExample.id);
  });

  it("lowers confidence as negative feedback accrues, excluding examples below threshold", () => {
    expect(adjustedConfidence(1, 0, 4)).toBeLessThan(
      adjustedConfidence(1, 0, 0)
    );

    const file = parseFile(RENAME_PATCH);
    const heavilyDownvoted = makeExample({
      selectedCode: RENAME_EXAMPLE_CODE,
      contextBefore: "const start = true;",
      contextAfter: "const end = true;",
      negativeFeedback: 4,
    });
    const drafts = callGenerate([file], [heavilyDownvoted]);
    expect(drafts).toHaveLength(0);
  });

  it("never suggests a disabled example", () => {
    const file = parseFile(RENAME_PATCH);
    const example = makeExample({
      selectedCode: RENAME_EXAMPLE_CODE,
      contextBefore: "const start = true;",
      contextAfter: "const end = true;",
      enabled: false,
    });
    expect(callGenerate([file], [example])).toHaveLength(0);
  });

  it("excludes examples sourced from the current session's comments", () => {
    const file = parseFile(RENAME_PATCH);
    const example = makeExample({
      selectedCode: RENAME_EXAMPLE_CODE,
      contextBefore: "const start = true;",
      contextAfter: "const end = true;",
      sourceCommentId: "comment-1",
    });
    const drafts = callGenerate([file], [example], {
      currentSessionCommentIds: new Set(["comment-1"]),
    });
    expect(drafts).toHaveLength(0);
  });

  it("excludes examples already suggested this session", () => {
    const file = parseFile(RENAME_PATCH);
    const example = makeExample({
      selectedCode: RENAME_EXAMPLE_CODE,
      contextBefore: "const start = true;",
      contextAfter: "const end = true;",
    });
    const drafts = callGenerate([file], [example], {
      alreadySuggestedExampleIds: new Set([example.id]),
    });
    expect(drafts).toHaveLength(0);
  });

  describe("import-only runs", () => {
    it("skips an import-only run for a non-import example", () => {
      const file = parseFile(IMPORT_PATCH);
      const example = makeExample({ selectedCode: RENAME_EXAMPLE_CODE });
      expect(callGenerate([file], [example])).toHaveLength(0);
    });

    it("matches an import-only run for an import-only example", () => {
      const file = parseFile(IMPORT_PATCH);
      const example = makeExample({
        selectedCode:
          'import { x } from "./x";\nimport { y } from "./y";\nimport { z } from "./z";',
      });
      const drafts = callGenerate([file], [example]);
      expect(drafts).toHaveLength(1);
    });
  });

  it("caps suggestions per file and reports explanation/proposedBody", () => {
    const file = parseFile(CAPS_PATCH);
    const letters = ["A", "B", "C", "D"];
    const examples = letters.map((letter) =>
      makeExample({
        selectedCode: `if (x === null) {\n  logger.warn${letter}();\n}`,
        commentBody: `Guard clause ${letter}`,
      })
    );

    const drafts = callGenerate([file], examples);

    expect(drafts).toHaveLength(MAX_SUGGESTIONS_PER_FILE);
    const [exampleA, exampleB, exampleC, exampleD] = examples;
    const draftedIds = drafts.map((draft) => draft.memoryExampleId);
    expect(draftedIds).toEqual([exampleA.id, exampleB.id, exampleC.id]);
    expect(draftedIds).not.toContain(exampleD.id);

    for (const draft of drafts) {
      expect(draft.anchor.path).toBe("src/caps.ts");
      expect(draft.explanation).toBe(
        "Similar to a comment you made previously"
      );
      const source = examples.find(
        (example) => example.id === draft.memoryExampleId
      );
      expect(draft.proposedBody).toBe(source?.commentBody);
    }
  });
});

describe("shouldCreateMemoryExample", () => {
  const SUBSTANTIVE_BODY =
    "This should use a Set instead of an array for O(1) lookups here.";
  const SUBSTANTIVE_CODE = RENAME_EXAMPLE_CODE;

  it("is true for a substantive comment on substantive, non-excluded code", () => {
    expect(
      shouldCreateMemoryExample({
        body: SUBSTANTIVE_BODY,
        selectedCode: SUBSTANTIVE_CODE,
        filePath: "src/app.ts",
      })
    ).toBe(true);
  });

  it.each([
    "nit",
    "why?",
    "fix this",
  ])("is false for the generic comment %s", (body) => {
    expect(
      shouldCreateMemoryExample({
        body,
        selectedCode: SUBSTANTIVE_CODE,
        filePath: "src/app.ts",
      })
    ).toBe(false);
  });

  it("is false for a short body", () => {
    expect(
      shouldCreateMemoryExample({
        body: "looks fine",
        selectedCode: SUBSTANTIVE_CODE,
        filePath: "src/app.ts",
      })
    ).toBe(false);
  });

  it("is false for lockfile paths", () => {
    expect(
      shouldCreateMemoryExample({
        body: SUBSTANTIVE_BODY,
        selectedCode: SUBSTANTIVE_CODE,
        filePath: "package-lock.json",
      })
    ).toBe(false);
  });

  it("is false for minified code", () => {
    const minified = `const a = "${"x".repeat(600)}";`;
    expect(
      shouldCreateMemoryExample({
        body: SUBSTANTIVE_BODY,
        selectedCode: minified,
        filePath: "src/app.ts",
      })
    ).toBe(false);
  });
});
