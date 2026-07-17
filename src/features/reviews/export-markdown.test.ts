import { describe, expect, it } from "vitest";
import type { DiffAnchor, ReviewComment } from "@/types/review";
import { buildReviewMarkdown } from "./export-markdown";

function anchor(overrides: Partial<DiffAnchor>): DiffAnchor {
  return {
    path: "src/a.ts",
    side: "new",
    startLine: 10,
    endLine: 10,
    baseRevision: "base",
    headRevision: "head",
    hunkHeader: "@@ -8,4 +8,4 @@",
    selectedCode: "const x = 1;",
    contextBefore: "",
    contextAfter: "",
    contextHash: "hash",
    ...overrides,
  };
}

function comment(overrides: Partial<ReviewComment>): ReviewComment {
  return {
    id: "c1",
    reviewSessionId: "s1",
    anchor: anchor({}),
    body: "please fix",
    language: "typescript",
    status: "draft",
    createdAt: "2026-07-11T00:00:00.000Z",
    updatedAt: "2026-07-11T00:00:00.000Z",
    ...overrides,
  };
}

describe("buildReviewMarkdown", () => {
  const input = {
    repoName: "lgtm",
    comparisonLabel: "Working tree",
    date: new Date("2026-07-11T12:00:00.000Z"),
    comments: [
      comment({ id: "c1" }),
      comment({
        id: "c2",
        status: "outdated",
        anchor: anchor({
          path: "src/b.ts",
          side: "old",
          startLine: 4,
          endLine: 6,
          selectedCode: "old();\ngone();",
        }),
        language: null,
        body: "was here",
      }),
    ],
  };

  it("groups comments by file with a dated header", () => {
    const md = buildReviewMarkdown(input);
    expect(md).toContain("# Review — lgtm");
    expect(md).toContain("Working tree · 2026-07-11");
    expect(md).toContain("### src/a.ts");
    expect(md).toContain("### src/b.ts");
    // File order follows first appearance.
    expect(md.indexOf("### src/a.ts")).toBeLessThan(md.indexOf("### src/b.ts"));
  });

  it("renders line ranges, code fences, and outdated markers", () => {
    const md = buildReviewMarkdown(input);
    expect(md).toContain("**line 10 (new)**");
    expect(md).toContain("```typescript");
    expect(md).toContain("const x = 1;");
    expect(md).toContain("**lines 4–6 (old)**");
    expect(md).toContain("_(outdated)_");
    expect(md).toContain("old();");
  });

  it("handles an empty review", () => {
    const md = buildReviewMarkdown({ ...input, comments: [] });
    expect(md).toContain("_No comments._");
  });
});
