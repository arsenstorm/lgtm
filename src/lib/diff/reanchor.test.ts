import { parsePatchFiles } from "@pierre/diffs";
import { describe, expect, it } from "vitest";
import { buildAnchor } from "./anchor";
import { reanchorAll, reanchorComment } from "./reanchor";

function parseFile(patch: string) {
  const parsed = parsePatchFiles(patch);
  const file = parsed[0]?.files[0];
  if (!file) {
    throw new Error("fixture patch failed to parse");
  }
  expect(file.hunks.length).toBeGreaterThan(0);
  return file;
}

function buildOrThrow(args: Parameters<typeof buildAnchor>[0]) {
  const anchor = buildAnchor(args);
  if (!anchor) {
    throw new Error("expected buildAnchor to succeed");
  }
  return anchor;
}

// Baseline fixture: a comment on `return fresh();` (line 6, additions side)
// and a second comment on `function helper() {}` (line 3, additions side).
const BASE_PATCH = `diff --git a/src/example.ts b/src/example.ts
index 1111111..2222222 100644
--- a/src/example.ts
+++ b/src/example.ts
@@ -1,9 +1,9 @@
 import a from "a";
 
 function helper() {}
 
 function foo() {
-  return old();
+  return fresh();
 }
 
 const tail = 1;
`;

describe("reanchorComment", () => {
  it("re-anchors at the same location when the refreshed patch is identical (tier 1)", () => {
    const oldFile = parseFile(BASE_PATCH);
    const anchor = buildOrThrow({
      file: oldFile,
      side: "additions",
      startLine: 6,
      endLine: 6,
      baseRevision: "base-1",
      headRevision: "head-1",
    });

    const newFile = parseFile(BASE_PATCH);
    const result = reanchorComment({
      anchor,
      file: newFile,
      baseRevision: "base-1",
      headRevision: "head-1",
    });

    expect(result.status).toBe("anchored");
    if (result.status !== "anchored") {
      throw new Error("unreachable");
    }
    expect(result.anchor.startLine).toBe(6);
    expect(result.anchor.endLine).toBe(6);
    expect(result.anchor.selectedCode).toBe(anchor.selectedCode);
    expect(result.anchor.contextHash).toBe(anchor.contextHash);
  });

  it("re-anchors when lines are inserted above the comment, shifting it down (tier 2)", () => {
    const oldFile = parseFile(BASE_PATCH);
    const anchor = buildOrThrow({
      file: oldFile,
      side: "additions",
      startLine: 6,
      endLine: 6,
      baseRevision: "base-1",
      headRevision: "head-1",
    });

    const shiftedPatch = `diff --git a/src/example.ts b/src/example.ts
index 1111111..2222222 100644
--- a/src/example.ts
+++ b/src/example.ts
@@ -1,9 +1,11 @@
+const zero = 0;
+const one = 1;
 import a from "a";
 
 function helper() {}
 
 function foo() {
-  return old();
+  return fresh();
 }
 
 const tail = 1;
`;
    const newFile = parseFile(shiftedPatch);
    const result = reanchorComment({
      anchor,
      file: newFile,
      baseRevision: "base-2",
      headRevision: "head-2",
    });

    expect(result.status).toBe("anchored");
    if (result.status !== "anchored") {
      throw new Error("unreachable");
    }
    expect(result.anchor.startLine).toBe(8);
    expect(result.anchor.endLine).toBe(8);
    expect(result.anchor.selectedCode).toBe(anchor.selectedCode);
  });

  it("marks the comment outdated when the code now appears twice with matching context (ambiguous)", () => {
    const oldFile = parseFile(BASE_PATCH);
    const anchor = buildOrThrow({
      file: oldFile,
      side: "additions",
      startLine: 6,
      endLine: 6,
      baseRevision: "base-1",
      headRevision: "head-1",
    });

    const duplicatedPatch = `diff --git a/src/example.ts b/src/example.ts
index 1111111..2222222 100644
--- a/src/example.ts
+++ b/src/example.ts
@@ -1,9 +1,19 @@
+const zero = 0;
+const one = 1;
 import a from "a";
 
 function helper() {}
 
 function foo() {
-  return old();
+  return fresh();
 }
 
 const tail = 1;
+
+function helper() {}
+
+function foo() {
+  return fresh();
+}
+
+const tail = 1;
`;
    const newFile = parseFile(duplicatedPatch);
    const result = reanchorComment({
      anchor,
      file: newFile,
      baseRevision: "base-2",
      headRevision: "head-2",
    });

    expect(result.status).toBe("outdated");
  });

  it("marks the comment outdated when the commented code was deleted", () => {
    const oldFile = parseFile(BASE_PATCH);
    const anchor = buildOrThrow({
      file: oldFile,
      side: "additions",
      startLine: 6,
      endLine: 6,
      baseRevision: "base-1",
      headRevision: "head-1",
    });

    const deletedPatch = `diff --git a/src/example.ts b/src/example.ts
index 1111111..2222222 100644
--- a/src/example.ts
+++ b/src/example.ts
@@ -1,9 +1,8 @@
 import a from "a";
 
 function helper() {}
 
 function foo() {
-  return old();
 }
 
 const tail = 1;
`;
    const newFile = parseFile(deletedPatch);
    const result = reanchorComment({
      anchor,
      file: newFile,
      baseRevision: "base-2",
      headRevision: "head-2",
    });

    expect(result.status).toBe("outdated");
  });

  it("re-anchors via a whitespace-insensitive match when the line was re-indented (tier 4)", () => {
    const oldFile = parseFile(BASE_PATCH);
    const anchor = buildOrThrow({
      file: oldFile,
      side: "additions",
      startLine: 6,
      endLine: 6,
      baseRevision: "base-1",
      headRevision: "head-1",
    });

    const reindentedPatch = `diff --git a/src/example.ts b/src/example.ts
index 1111111..2222222 100644
--- a/src/example.ts
+++ b/src/example.ts
@@ -1,9 +1,9 @@
 import a from "a";
 
 function helper() {}
 
 function foo() {
-  return old();
+    return fresh();
 }
 
 const tail = 1;
`;
    const newFile = parseFile(reindentedPatch);
    const result = reanchorComment({
      anchor,
      file: newFile,
      baseRevision: "base-2",
      headRevision: "head-2",
    });

    expect(result.status).toBe("anchored");
    if (result.status !== "anchored") {
      throw new Error("unreachable");
    }
    // NOTE: tier 4 returns the *new* indentation, not the originally
    // commented-on text — the anchor is refreshed to match reality.
    expect(result.anchor.selectedCode).toBe("    return fresh();");
    expect(result.anchor.selectedCode).not.toBe(anchor.selectedCode);
  });

  it("marks the comment outdated when the code was materially edited (different tokens)", () => {
    const oldFile = parseFile(BASE_PATCH);
    const anchor = buildOrThrow({
      file: oldFile,
      side: "additions",
      startLine: 6,
      endLine: 6,
      baseRevision: "base-1",
      headRevision: "head-1",
    });

    const editedPatch = `diff --git a/src/example.ts b/src/example.ts
index 1111111..2222222 100644
--- a/src/example.ts
+++ b/src/example.ts
@@ -1,9 +1,9 @@
 import a from "a";
 
 function helper() {}
 
 function foo() {
-  return old();
+  return fresh(extra);
 }
 
 const tail = 1;
`;
    const newFile = parseFile(editedPatch);
    const result = reanchorComment({
      anchor,
      file: newFile,
      baseRevision: "base-2",
      headRevision: "head-2",
    });

    expect(result.status).toBe("outdated");
  });

  it("preserves the multi-line span when a 3-line anchor shifts down", () => {
    const multiLinePatch = `diff --git a/src/calc.ts b/src/calc.ts
index 3333333..4444444 100644
--- a/src/calc.ts
+++ b/src/calc.ts
@@ -1,5 +1,7 @@
 const a = 1;
 function calc() {
-  return 0;
+  const x = 1;
+  const y = 2;
+  return x + y;
 }
 const b = 2;
`;
    const oldFile = parseFile(multiLinePatch);
    const anchor = buildOrThrow({
      file: oldFile,
      side: "additions",
      startLine: 3,
      endLine: 5,
      baseRevision: "base-1",
      headRevision: "head-1",
    });
    expect(anchor.selectedCode).toBe(
      "  const x = 1;\n  const y = 2;\n  return x + y;"
    );

    const shiftedPatch = `diff --git a/src/calc.ts b/src/calc.ts
index 3333333..4444444 100644
--- a/src/calc.ts
+++ b/src/calc.ts
@@ -1,5 +1,9 @@
+const zero = 0;
+const one = 1;
 const a = 1;
 function calc() {
-  return 0;
+  const x = 1;
+  const y = 2;
+  return x + y;
 }
 const b = 2;
`;
    const newFile = parseFile(shiftedPatch);
    const result = reanchorComment({
      anchor,
      file: newFile,
      baseRevision: "base-2",
      headRevision: "head-2",
    });

    expect(result.status).toBe("anchored");
    if (result.status !== "anchored") {
      throw new Error("unreachable");
    }
    expect(result.anchor.startLine).toBe(5);
    expect(result.anchor.endLine).toBe(7);
    expect(result.anchor.endLine - result.anchor.startLine).toBe(
      anchor.endLine - anchor.startLine
    );
    expect(result.anchor.selectedCode).toBe(anchor.selectedCode);
  });

  it("re-anchors an old-side (deletions) anchor when the deletion still exists", () => {
    const deletionPatch = `diff --git a/src/other.ts b/src/other.ts
index 5555555..6666666 100644
--- a/src/other.ts
+++ b/src/other.ts
@@ -1,4 +1,4 @@
 const a = 1;
-const b = 2;
+const b = 20;
 const c = 3;
 const d = 4;
`;
    const oldFile = parseFile(deletionPatch);
    const anchor = buildOrThrow({
      file: oldFile,
      side: "deletions",
      startLine: 2,
      endLine: 2,
      baseRevision: "base-1",
      headRevision: "head-1",
    });
    expect(anchor.side).toBe("old");
    expect(anchor.selectedCode).toBe("const b = 2;");

    const refreshedPatch = `diff --git a/src/other.ts b/src/other.ts
index 5555555..7777777 100644
--- a/src/other.ts
+++ b/src/other.ts
@@ -1,4 +1,4 @@
 const a = 1;
-const b = 2;
+const b = 200;
 const c = 3;
 const d = 4;
`;
    const newFile = parseFile(refreshedPatch);
    const result = reanchorComment({
      anchor,
      file: newFile,
      baseRevision: "base-2",
      headRevision: "head-2",
    });

    expect(result.status).toBe("anchored");
    if (result.status !== "anchored") {
      throw new Error("unreachable");
    }
    expect(result.anchor.side).toBe("old");
    expect(result.anchor.startLine).toBe(2);
    expect(result.anchor.selectedCode).toBe("const b = 2;");
  });

  it("carries the new baseRevision/headRevision instead of the original anchor's", () => {
    const oldFile = parseFile(BASE_PATCH);
    const anchor = buildOrThrow({
      file: oldFile,
      side: "additions",
      startLine: 6,
      endLine: 6,
      baseRevision: "base-1",
      headRevision: "head-1",
    });

    const newFile = parseFile(BASE_PATCH);
    const result = reanchorComment({
      anchor,
      file: newFile,
      baseRevision: "base-2",
      headRevision: "head-2",
    });

    expect(result.status).toBe("anchored");
    if (result.status !== "anchored") {
      throw new Error("unreachable");
    }
    expect(result.anchor.baseRevision).toBe("base-2");
    expect(result.anchor.headRevision).toBe("head-2");
    expect(result.anchor.baseRevision).not.toBe(anchor.baseRevision);
    expect(result.anchor.headRevision).not.toBe(anchor.headRevision);
  });
});

describe("reanchorAll", () => {
  it("splits comments into anchored and outdated, and treats a missing file as all-outdated", () => {
    const oldFile = parseFile(BASE_PATCH);
    const liveAnchor = buildOrThrow({
      file: oldFile,
      side: "additions",
      startLine: 6,
      endLine: 6,
      baseRevision: "base-1",
      headRevision: "head-1",
    });
    const deadAnchor = buildOrThrow({
      file: oldFile,
      side: "additions",
      startLine: 3,
      endLine: 3,
      baseRevision: "base-1",
      headRevision: "head-1",
    });

    const renamedPatch = `diff --git a/src/example.ts b/src/example.ts
index 1111111..2222222 100644
--- a/src/example.ts
+++ b/src/example.ts
@@ -1,9 +1,9 @@
 import a from "a";
 
-function helper() {}
+function utility() {}
 
 function foo() {
-  return old();
+  return fresh();
 }
 
 const tail = 1;
`;
    const newFile = parseFile(renamedPatch);

    const liveComment = { id: "live", anchor: liveAnchor };
    const deadComment = { id: "dead", anchor: deadAnchor };

    const { anchored, outdated } = reanchorAll({
      comments: [liveComment, deadComment],
      file: newFile,
      baseRevision: "base-2",
      headRevision: "head-2",
    });

    expect(anchored).toHaveLength(1);
    expect(anchored[0]?.comment.id).toBe("live");
    expect(anchored[0]?.anchor.selectedCode).toBe("  return fresh();");
    expect(outdated).toHaveLength(1);
    expect(outdated[0]?.id).toBe("dead");

    const missingFileResult = reanchorAll({
      comments: [liveComment, deadComment],
      file: undefined,
      baseRevision: "base-2",
      headRevision: "head-2",
    });
    expect(missingFileResult.anchored).toHaveLength(0);
    expect(missingFileResult.outdated).toHaveLength(2);
  });
});
