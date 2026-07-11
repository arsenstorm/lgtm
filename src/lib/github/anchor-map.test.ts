import { describe, expect, it } from "vitest";
import type { DiffAnchor } from "../../types/review";
import { anchorToGithubComment } from "./anchor-map";

const baseAnchor: DiffAnchor = {
  path: "src/foo.ts",
  side: "new",
  startLine: 10,
  endLine: 10,
  baseRevision: "base-sha",
  headRevision: "head-sha",
  hunkHeader: "@@ -8,4 +8,4 @@",
  selectedCode: "const x = 1;",
  contextBefore: "",
  contextAfter: "",
  contextHash: "hash",
};

describe("anchorToGithubComment", () => {
  it("maps a single-line new-side anchor to RIGHT with no startLine", () => {
    const draft = anchorToGithubComment(baseAnchor, "please fix");

    expect(draft).toEqual({
      path: "src/foo.ts",
      body: "please fix",
      line: 10,
      side: "RIGHT",
    });
    expect(draft.startLine).toBeUndefined();
    expect(draft.startSide).toBeUndefined();
  });

  it("maps a multi-line old-side anchor to LEFT with startLine/startSide", () => {
    const anchor: DiffAnchor = {
      ...baseAnchor,
      side: "old",
      startLine: 5,
      endLine: 8,
    };

    const draft = anchorToGithubComment(anchor, "consider extracting this");

    expect(draft).toEqual({
      path: "src/foo.ts",
      body: "consider extracting this",
      line: 8,
      side: "LEFT",
      startLine: 5,
      startSide: "LEFT",
    });
  });

  it("passes the body through unchanged", () => {
    const draft = anchorToGithubComment(baseAnchor, "some body text");

    expect(draft.body).toBe("some body text");
  });
});
