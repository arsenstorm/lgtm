import type { GithubReviewCommentDraft, GithubSide } from "../../types/github";
import type { DiffAnchor } from "../../types/review";

/** Maps a domain anchor side to GitHub's review-comment side vocabulary. */
export function anchorSideToGithubSide(side: DiffAnchor["side"]): GithubSide {
  return side === "new" ? "RIGHT" : "LEFT";
}

/**
 * Converts a draft comment's anchor into GitHub's grouped-review comment
 * shape. GitHub wants the END line in `line` and, for multi-line comments,
 * the start in `startLine`/`startSide` (same side — LGTM anchors never span
 * sides).
 */
export function anchorToGithubComment(
  anchor: DiffAnchor,
  body: string
): GithubReviewCommentDraft {
  const side = anchorSideToGithubSide(anchor.side);
  const draft: GithubReviewCommentDraft = {
    path: anchor.path,
    body,
    line: anchor.endLine,
    side,
  };
  if (anchor.startLine !== anchor.endLine) {
    draft.startLine = anchor.startLine;
    draft.startSide = side;
  }
  return draft;
}
