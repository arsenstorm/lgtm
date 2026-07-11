import type { FileDiffMetadata } from "@pierre/diffs";
import type { DiffAnchor } from "../../types/review";
import { anchorSideToPatchSide, buildAnchor } from "./anchor";
import { listSideLines, sliceLines } from "./patch-lines";

export type ReanchorResult =
  | { status: "anchored"; anchor: DiffAnchor }
  | { status: "outdated" };

const OUTDATED: ReanchorResult = { status: "outdated" };

function normalizeLoose(code: string): string {
  return code
    .split("\n")
    .map((line) => line.trim())
    .join("\n");
}

/**
 * Attempts to re-anchor a comment onto a refreshed diff of the same file.
 *
 * Tiers, most to least reliable — the first hit wins, anything else is
 * conservatively marked outdated rather than silently moved:
 * 1. Same side/lines still hold the same code and context (context hash).
 * 2. Exactly one position in the new diff has identical selected code AND a
 *    matching context hash.
 * 3. Exactly one position has identical selected code (context changed).
 * 4. Exactly one position matches the selected code ignoring leading and
 *    trailing whitespace per line (e.g. the file was re-indented).
 */
export function reanchorComment(args: {
  anchor: DiffAnchor;
  file: FileDiffMetadata;
  baseRevision: string;
  headRevision: string;
}): ReanchorResult {
  const { anchor, file } = args;
  const side = anchorSideToPatchSide(anchor.side);
  const length = anchor.endLine - anchor.startLine;

  const rebuild = (startLine: number): DiffAnchor | null =>
    buildAnchor({
      file,
      side,
      startLine,
      endLine: startLine + length,
      baseRevision: args.baseRevision,
      headRevision: args.headRevision,
    });

  // Tier 1: unchanged location.
  const inPlace = rebuild(anchor.startLine);
  if (
    inPlace &&
    inPlace.selectedCode === anchor.selectedCode &&
    inPlace.contextHash === anchor.contextHash
  ) {
    return { status: "anchored", anchor: inPlace };
  }

  // Collect candidate start lines whose slice matches the selected code.
  const exact: number[] = [];
  const loose: number[] = [];
  const wantedLoose = normalizeLoose(anchor.selectedCode);
  for (const startLine of listSideLines(file, side)) {
    const slice = sliceLines(file, side, startLine, startLine + length);
    if (!slice) {
      continue;
    }
    const code = slice.join("\n");
    if (code === anchor.selectedCode) {
      exact.push(startLine);
    } else if (normalizeLoose(code) === wantedLoose) {
      loose.push(startLine);
    }
  }

  // Tier 2: unique exact-code match with matching context.
  const contextMatches = exact.filter((startLine) => {
    const candidate = rebuild(startLine);
    return candidate?.contextHash === anchor.contextHash;
  });
  if (contextMatches.length === 1) {
    const candidate = rebuild(contextMatches[0]);
    if (candidate) {
      return { status: "anchored", anchor: candidate };
    }
  }
  if (contextMatches.length > 1) {
    return OUTDATED;
  }

  // Tier 3: unique exact-code match, context changed.
  if (exact.length === 1) {
    const candidate = rebuild(exact[0]);
    if (candidate) {
      return { status: "anchored", anchor: candidate };
    }
  }
  if (exact.length > 1) {
    return OUTDATED;
  }

  // Tier 4: unique whitespace-insensitive match.
  if (loose.length === 1) {
    const candidate = rebuild(loose[0]);
    if (candidate) {
      return { status: "anchored", anchor: candidate };
    }
  }

  return OUTDATED;
}

/**
 * Re-anchors every comment for one file. Comments whose anchor cannot be
 * re-established reliably are returned in `outdated`.
 */
export function reanchorAll<T extends { anchor: DiffAnchor }>(args: {
  comments: T[];
  file: FileDiffMetadata | undefined;
  baseRevision: string;
  headRevision: string;
}): { anchored: { comment: T; anchor: DiffAnchor }[]; outdated: T[] } {
  const anchored: { comment: T; anchor: DiffAnchor }[] = [];
  const outdated: T[] = [];
  for (const comment of args.comments) {
    if (!args.file) {
      outdated.push(comment);
      continue;
    }
    const result = reanchorComment({
      anchor: comment.anchor,
      file: args.file,
      baseRevision: args.baseRevision,
      headRevision: args.headRevision,
    });
    if (result.status === "anchored") {
      anchored.push({ comment, anchor: result.anchor });
    } else {
      outdated.push(comment);
    }
  }
  return { anchored, outdated };
}
