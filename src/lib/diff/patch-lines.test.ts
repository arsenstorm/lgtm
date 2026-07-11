import { parsePatchFiles } from "@pierre/diffs";
import { describe, expect, it } from "vitest";
import { buildAnchor, contextHash } from "./anchor";
import { listSideLines, locateLine, sliceLines, stripEol } from "./patch-lines";

const HEX_HASH = /^[0-9a-f]{16}$/;

const PATCH = `diff --git a/src/example.ts b/src/example.ts
index 1111111..2222222 100644
--- a/src/example.ts
+++ b/src/example.ts
@@ -1,7 +1,8 @@
 const a = 1;
-const b = 2;
+const b = 20;
+const b2 = 21;
 const c = 3;
 const d = 4;
 const e = 5;
 const f = 6;
 const g = 7;
@@ -20,3 +21,4 @@
 function tail() {
-  return old();
+  return fresh();
+  // extra
 }
`;

function parseFixture() {
  const parsed = parsePatchFiles(PATCH);
  const file = parsed[0]?.files[0];
  if (!file) {
    throw new Error("fixture patch failed to parse");
  }
  return file;
}

describe("locateLine", () => {
  it("locates an added line", () => {
    const file = parseFixture();
    const location = locateLine(file, "additions", 2);
    expect(location).not.toBeNull();
    expect(location?.blockType).toBe("change");
    expect(stripEol(file.additionLines[location?.arrayIndex ?? -1] ?? "")).toBe(
      "const b = 20;"
    );
  });

  it("locates a deleted line", () => {
    const file = parseFixture();
    const location = locateLine(file, "deletions", 2);
    expect(location?.blockType).toBe("change");
    expect(stripEol(file.deletionLines[location?.arrayIndex ?? -1] ?? "")).toBe(
      "const b = 2;"
    );
  });

  it("locates a context line on the additions side", () => {
    const file = parseFixture();
    const location = locateLine(file, "additions", 4);
    expect(location?.blockType).toBe("context");
    expect(stripEol(file.additionLines[location?.arrayIndex ?? -1] ?? "")).toBe(
      "const c = 3;"
    );
  });

  it("returns null for lines outside every hunk", () => {
    const file = parseFixture();
    expect(locateLine(file, "additions", 15)).toBeNull();
    expect(locateLine(file, "deletions", 100)).toBeNull();
  });

  it("locates lines in a later hunk", () => {
    const file = parseFixture();
    const location = locateLine(file, "additions", 22);
    expect(location?.hunkIndex).toBe(1);
    expect(stripEol(file.additionLines[location?.arrayIndex ?? -1] ?? "")).toBe(
      "  return fresh();"
    );
  });
});

describe("sliceLines", () => {
  it("slices a contiguous added range", () => {
    const file = parseFixture();
    expect(sliceLines(file, "additions", 2, 3)).toEqual([
      "const b = 20;",
      "const b2 = 21;",
    ]);
  });

  it("returns null for a range spanning hunks", () => {
    const file = parseFixture();
    expect(sliceLines(file, "additions", 7, 22)).toBeNull();
  });
});

describe("listSideLines", () => {
  it("enumerates addition-side lines across hunks", () => {
    const file = parseFixture();
    const lines = listSideLines(file, "additions");
    expect(lines).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 21, 22, 23, 24]);
  });
});

describe("buildAnchor", () => {
  it("builds an anchor for an added range with context", () => {
    const file = parseFixture();
    const anchor = buildAnchor({
      file,
      side: "additions",
      startLine: 2,
      endLine: 3,
      baseRevision: "base-sha",
      headRevision: "head-sha",
    });
    expect(anchor).not.toBeNull();
    expect(anchor?.side).toBe("new");
    expect(anchor?.path).toBe("src/example.ts");
    expect(anchor?.selectedCode).toBe("const b = 20;\nconst b2 = 21;");
    expect(anchor?.contextBefore).toBe("const a = 1;");
    expect(anchor?.contextAfter).toBe(
      "const c = 3;\nconst d = 4;\nconst e = 5;"
    );
    expect(anchor?.hunkHeader).toContain("@@ -1,7 +1,8 @@");
    expect(anchor?.contextHash).toMatch(HEX_HASH);
  });

  it("builds an old-side anchor", () => {
    const file = parseFixture();
    const anchor = buildAnchor({
      file,
      side: "deletions",
      startLine: 2,
      endLine: 2,
      baseRevision: "base-sha",
      headRevision: "head-sha",
    });
    expect(anchor?.side).toBe("old");
    expect(anchor?.selectedCode).toBe("const b = 2;");
  });

  it("normalises reversed ranges", () => {
    const file = parseFixture();
    const anchor = buildAnchor({
      file,
      side: "additions",
      startLine: 3,
      endLine: 2,
      baseRevision: "b",
      headRevision: "h",
    });
    expect(anchor?.startLine).toBe(2);
    expect(anchor?.endLine).toBe(3);
  });

  it("refuses ranges that cross hunks", () => {
    const file = parseFixture();
    const anchor = buildAnchor({
      file,
      side: "additions",
      startLine: 7,
      endLine: 22,
      baseRevision: "b",
      headRevision: "h",
    });
    expect(anchor).toBeNull();
  });
});

describe("contextHash", () => {
  it("ignores trailing whitespace", () => {
    expect(contextHash("a", "b  ", "c")).toBe(contextHash("a", "b", "c"));
  });

  it("changes when the selection changes", () => {
    expect(contextHash("a", "b", "c")).not.toBe(contextHash("a", "x", "c"));
  });

  it("keeps segment boundaries distinct", () => {
    expect(contextHash("a b", "c", "")).not.toBe(contextHash("a", "b c", ""));
  });
});
