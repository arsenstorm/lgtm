import { parsePatchFiles } from "@pierre/diffs";
import { describe, expect, it } from "vitest";
import { collectAdditionRuns, windowsForExample } from "./candidates";

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

describe("collectAdditionRuns", () => {
  it("collects contiguous added-line runs per hunk", () => {
    const file = parseFixture();
    const runs = collectAdditionRuns(file);
    expect(runs).toEqual([
      { startLine: 2, lines: ["const b = 20;", "const b2 = 21;"] },
      { startLine: 22, lines: ["  return fresh();", "  // extra"] },
    ]);
  });

  it("returns no runs for a file with only context lines", () => {
    const patch = `diff --git a/a.ts b/a.ts
index 1111111..1111112 100644
--- a/a.ts
+++ b/a.ts
@@ -1,1 +1,1 @@
 const a = 1;
`;
    const parsed = parsePatchFiles(patch);
    const file = parsed[0]?.files[0];
    if (!file) {
      throw new Error("fixture patch failed to parse");
    }
    expect(collectAdditionRuns(file)).toEqual([]);
  });
});

describe("windowsForExample", () => {
  it("returns the whole run when it is small relative to the example", () => {
    const run = { startLine: 10, lines: ["a", "b", "c"] };
    const windows = windowsForExample(run, 5);
    expect(windows).toEqual([{ startLine: 10, endLine: 12, code: "a\nb\nc" }]);
  });

  it("returns nothing when the run is far smaller than half the example size", () => {
    const run = { startLine: 10, lines: ["a"] };
    const windows = windowsForExample(run, 10);
    expect(windows).toEqual([]);
  });

  it("slides a window across a run longer than the example", () => {
    const run = { startLine: 1, lines: ["1", "2", "3", "4", "5", "6"] };
    const windows = windowsForExample(run, 4);
    // size=4, step=floor(4/2)=2 -> offsets 0, 2
    expect(windows).toEqual([
      { startLine: 1, endLine: 4, code: "1\n2\n3\n4" },
      { startLine: 3, endLine: 6, code: "3\n4\n5\n6" },
    ]);
  });

  it("caps the number of windows produced for a very long run", () => {
    const lines = Array.from({ length: 200 }, (_, i) => String(i));
    const run = { startLine: 1, lines };
    const windows = windowsForExample(run, 4);
    expect(windows.length).toBeLessThanOrEqual(40);
  });
});
