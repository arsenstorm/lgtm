import type { GithubReviewEvent } from "@/types/github";

/**
 * Whether a review may be submitted. GitHub rejects a COMMENT review with no
 * body and no comments; REQUEST_CHANGES needs the same substance. APPROVE is
 * always allowed (an empty approval is valid).
 */
export function canSubmit(
  event: GithubReviewEvent,
  body: string,
  draftCount: number
): boolean {
  if (event === "APPROVE") {
    return true;
  }
  return draftCount > 0 || body.trim().length > 0;
}
