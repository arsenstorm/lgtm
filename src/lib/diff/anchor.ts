import type { FileDiffMetadata } from "@pierre/diffs";
import type { DiffAnchor } from "../../types/review";
import {
  locateLine,
  type PatchSide,
  sliceLines,
  stripEol,
} from "./patch-lines";

const CONTEXT_LINES = 3;

const FNV_OFFSET_BASIS = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const U64_MASK = 0xffffffffffffffffn;
const HEX_LENGTH = 16;

function fnv1a64(text: string): string {
  let hash = FNV_OFFSET_BASIS;
  for (let i = 0; i < text.length; i++) {
    // biome-ignore lint/suspicious/noBitwiseOperators: FNV-1a is a bitwise hash by definition.
    hash ^= BigInt(text.charCodeAt(i));
    // biome-ignore lint/suspicious/noBitwiseOperators: wrap multiplication to 64 bits.
    hash = (hash * FNV_PRIME) & U64_MASK;
  }
  return hash.toString(16).padStart(HEX_LENGTH, "0");
}

function normalizeForHash(text: string): string {
  return text
    .split("\n")
    .map((line) => line.trimEnd())
    .join("\n");
}

/**
 * Stable hash of a comment's selected code plus surrounding context, used to
 * recognise the same location after a diff refresh.
 */
export function contextHash(
  before: string,
  selected: string,
  after: string
): string {
  return fnv1a64(
    `${normalizeForHash(before)}\u0000${normalizeForHash(selected)}\u0000${normalizeForHash(after)}`
  );
}

export type BuildAnchorArgs = {
  file: FileDiffMetadata;
  side: PatchSide;
  startLine: number;
  endLine: number;
  baseRevision: string;
  headRevision: string;
};

/**
 * Converts a renderer selection (side + one-based file line range) into a
 * durable DiffAnchor. Returns null when the range is not fully contained in
 * a single hunk — selections across collapsed regions cannot be anchored.
 */
export function buildAnchor(args: BuildAnchorArgs): DiffAnchor | null {
  const { file, side } = args;
  const startLine = Math.min(args.startLine, args.endLine);
  const endLine = Math.max(args.startLine, args.endLine);

  const start = locateLine(file, side, startLine);
  const end = locateLine(file, side, endLine);
  if (!(start && end) || start.hunkIndex !== end.hunkIndex) {
    return null;
  }
  const selected = sliceLines(file, side, startLine, endLine);
  if (!selected) {
    return null;
  }

  const hunk = file.hunks[start.hunkIndex];
  const lines = side === "additions" ? file.additionLines : file.deletionLines;
  const hunkStartIndex =
    side === "additions" ? hunk.additionLineIndex : hunk.deletionLineIndex;
  const hunkLineCount =
    side === "additions" ? hunk.additionCount : hunk.deletionCount;
  const hunkEndIndex = hunkStartIndex + hunkLineCount;

  const beforeStart = Math.max(
    hunkStartIndex,
    start.arrayIndex - CONTEXT_LINES
  );
  const afterEnd = Math.min(hunkEndIndex, end.arrayIndex + 1 + CONTEXT_LINES);
  const contextBefore = lines
    .slice(beforeStart, start.arrayIndex)
    .map(stripEol)
    .join("\n");
  const contextAfter = lines
    .slice(end.arrayIndex + 1, afterEnd)
    .map(stripEol)
    .join("\n");
  const selectedCode = selected.join("\n");

  return {
    path: file.name,
    side: side === "additions" ? "new" : "old",
    startLine,
    endLine,
    baseRevision: args.baseRevision,
    headRevision: args.headRevision,
    hunkHeader: hunk.hunkSpecs ?? "",
    selectedCode,
    contextBefore,
    contextAfter,
    contextHash: contextHash(contextBefore, selectedCode, contextAfter),
  };
}

/** Maps a domain anchor side back to the renderer's side vocabulary. */
export function anchorSideToPatchSide(side: DiffAnchor["side"]): PatchSide {
  return side === "new" ? "additions" : "deletions";
}
