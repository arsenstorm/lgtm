import type { PrInlineComment } from "../../types/github";
import type { PatchSide } from "../diff/patch-lines";

export type InlineCommentPlacement = { side: PatchSide; lineNumber: number };

/**
 * Where an existing GitHub review comment renders in the current diff.
 * Returns null for comments GitHub reports without a current line (outdated
 * against the latest head) and for replies (rendered under their parent).
 */
export function placeInlineComment(
  comment: PrInlineComment
): InlineCommentPlacement | null {
  if (comment.inReplyToId !== null || comment.line === null) {
    return null;
  }
  return {
    side: comment.side === "LEFT" ? "deletions" : "additions",
    lineNumber: comment.line,
  };
}

/** Groups top-level comments with their replies, in creation order. */
export function threadInlineComments(
  comments: PrInlineComment[]
): Map<number, PrInlineComment[]> {
  const sorted = [...comments].sort((a, b) =>
    a.createdAt.localeCompare(b.createdAt)
  );
  const threads = new Map<number, PrInlineComment[]>();
  for (const comment of sorted) {
    if (comment.inReplyToId === null) {
      if (!threads.has(comment.id)) {
        threads.set(comment.id, [comment]);
      }
    } else {
      const thread = threads.get(comment.inReplyToId);
      if (thread) {
        thread.push(comment);
      } else {
        // Parent missing from the page — surface the reply as its own thread.
        threads.set(comment.id, [comment]);
      }
    }
  }
  return threads;
}
