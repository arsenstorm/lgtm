import { describe, expect, it } from "vitest";
import type { PrInlineComment } from "../../types/github";
import { placeInlineComment, threadInlineComments } from "./inline-comment-map";

const baseComment: PrInlineComment = {
  id: 1,
  authorLogin: "octocat",
  path: "src/foo.ts",
  line: 10,
  originalLine: 10,
  side: "RIGHT",
  body: "looks good",
  createdAt: "2024-01-01T00:00:00Z",
  htmlUrl: "https://github.com/o/r/pull/1#discussion_r1",
  inReplyToId: null,
};

describe("placeInlineComment", () => {
  it("maps LEFT side to deletions", () => {
    const placement = placeInlineComment({ ...baseComment, side: "LEFT" });

    expect(placement).toEqual({ side: "deletions", lineNumber: 10 });
  });

  it("maps RIGHT side to additions", () => {
    const placement = placeInlineComment({ ...baseComment, side: "RIGHT" });

    expect(placement).toEqual({ side: "additions", lineNumber: 10 });
  });

  it("maps null side to additions", () => {
    const placement = placeInlineComment({ ...baseComment, side: null });

    expect(placement).toEqual({ side: "additions", lineNumber: 10 });
  });

  it("returns null when line is null", () => {
    const placement = placeInlineComment({ ...baseComment, line: null });

    expect(placement).toBeNull();
  });

  it("returns null for a reply", () => {
    const placement = placeInlineComment({ ...baseComment, inReplyToId: 99 });

    expect(placement).toBeNull();
  });
});

describe("threadInlineComments", () => {
  it("groups replies under their parent in createdAt order", () => {
    const parent: PrInlineComment = {
      ...baseComment,
      id: 1,
      createdAt: "2024-01-01T00:00:00Z",
    };
    const replyLater: PrInlineComment = {
      ...baseComment,
      id: 2,
      createdAt: "2024-01-03T00:00:00Z",
      inReplyToId: 1,
    };
    const replyEarlier: PrInlineComment = {
      ...baseComment,
      id: 3,
      createdAt: "2024-01-02T00:00:00Z",
      inReplyToId: 1,
    };

    const threads = threadInlineComments([parent, replyLater, replyEarlier]);

    expect(threads.size).toBe(1);
    expect(threads.get(1)).toEqual([parent, replyEarlier, replyLater]);
  });

  it("surfaces an orphan reply as its own thread", () => {
    const orphanReply: PrInlineComment = {
      ...baseComment,
      id: 2,
      inReplyToId: 999,
    };

    const threads = threadInlineComments([orphanReply]);

    expect(threads.size).toBe(1);
    expect(threads.get(2)).toEqual([orphanReply]);
  });

  it("puts two top-level comments in two threads", () => {
    const first: PrInlineComment = { ...baseComment, id: 1 };
    const second: PrInlineComment = { ...baseComment, id: 2 };

    const threads = threadInlineComments([first, second]);

    expect(threads.size).toBe(2);
    expect(threads.get(1)).toEqual([first]);
    expect(threads.get(2)).toEqual([second]);
  });
});
