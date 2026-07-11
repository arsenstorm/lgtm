import type { DiffAnchor } from "@/types/review";

/** Human label for an anchor side: "new" (additions) or "old" (deletions). */
export function sideLabel(side: DiffAnchor["side"]): string {
  return side === "new" ? "new" : "old";
}

/**
 * Renders a one-based line range as a caption, e.g. "line 12 (new)" or
 * "lines 12–15 (old)". Uses an en dash to match the surrounding UI.
 */
export function describeAnchorRange(
  startLine: number,
  endLine: number,
  side: DiffAnchor["side"]
): string {
  const label = sideLabel(side);
  if (startLine === endLine) {
    return `line ${startLine} (${label})`;
  }
  return `lines ${startLine}–${endLine} (${label})`;
}
