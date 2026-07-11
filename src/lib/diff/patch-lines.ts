import type { FileDiffMetadata, Hunk } from "@pierre/diffs";

export type PatchSide = "deletions" | "additions";

/**
 * @pierre/diffs keeps the trailing newline (and \r\n) on every entry of
 * additionLines/deletionLines. Strip it when lifting lines into domain data.
 */
const TRAILING_EOL = /\r?\n$/;

export function stripEol(line: string): string {
  return line.replace(TRAILING_EOL, "");
}

export type LineLocation = {
  hunkIndex: number;
  /** Index into FileDiffMetadata.additionLines or .deletionLines. */
  arrayIndex: number;
  blockType: "context" | "change";
};

/**
 * Maps a one-based file line number on one side of a diff to its position in
 * the parsed patch arrays. Returns null when the line is not part of the
 * patch (it falls in collapsed, unchanged territory).
 */
export function locateLine(
  file: FileDiffMetadata,
  side: PatchSide,
  lineNumber: number
): LineLocation | null {
  for (const [hunkIndex, hunk] of file.hunks.entries()) {
    const location = locateInHunk(hunk, hunkIndex, side, lineNumber);
    if (location) {
      return location;
    }
  }
  return null;
}

function locateInHunk(
  hunk: Hunk,
  hunkIndex: number,
  side: PatchSide,
  lineNumber: number
): LineLocation | null {
  let additionLine = hunk.additionStart;
  let deletionLine = hunk.deletionStart;

  for (const block of hunk.hunkContent) {
    if (block.type === "context") {
      if (
        side === "additions" &&
        lineNumber >= additionLine &&
        lineNumber < additionLine + block.lines
      ) {
        return {
          hunkIndex,
          arrayIndex: block.additionLineIndex + (lineNumber - additionLine),
          blockType: "context",
        };
      }
      if (
        side === "deletions" &&
        lineNumber >= deletionLine &&
        lineNumber < deletionLine + block.lines
      ) {
        return {
          hunkIndex,
          arrayIndex: block.deletionLineIndex + (lineNumber - deletionLine),
          blockType: "context",
        };
      }
      additionLine += block.lines;
      deletionLine += block.lines;
    } else {
      if (
        side === "deletions" &&
        lineNumber >= deletionLine &&
        lineNumber < deletionLine + block.deletions
      ) {
        return {
          hunkIndex,
          arrayIndex: block.deletionLineIndex + (lineNumber - deletionLine),
          blockType: "change",
        };
      }
      if (
        side === "additions" &&
        lineNumber >= additionLine &&
        lineNumber < additionLine + block.additions
      ) {
        return {
          hunkIndex,
          arrayIndex: block.additionLineIndex + (lineNumber - additionLine),
          blockType: "change",
        };
      }
      deletionLine += block.deletions;
      additionLine += block.additions;
    }
  }
  return null;
}

/**
 * Returns the text of the inclusive line range on one side, or null when the
 * range is not fully contained in a single hunk. Within one hunk,
 * consecutive file lines on a side are contiguous in the patch arrays, so a
 * matching index delta proves the range has no gaps.
 */
export function sliceLines(
  file: FileDiffMetadata,
  side: PatchSide,
  startLine: number,
  endLine: number
): string[] | null {
  const start = locateLine(file, side, startLine);
  const end = locateLine(file, side, endLine);
  if (!(start && end) || start.hunkIndex !== end.hunkIndex) {
    return null;
  }
  if (end.arrayIndex - start.arrayIndex !== endLine - startLine) {
    return null;
  }
  const lines = side === "additions" ? file.additionLines : file.deletionLines;
  return lines.slice(start.arrayIndex, end.arrayIndex + 1).map(stripEol);
}

/**
 * Enumerates every file line number present in the patch for one side, in
 * ascending order. Used by re-anchoring to scan candidate positions.
 */
export function listSideLines(
  file: FileDiffMetadata,
  side: PatchSide
): number[] {
  const result: number[] = [];
  for (const hunk of file.hunks) {
    pushHunkSideLines(hunk, side, result);
  }
  return result;
}

function pushHunkSideLines(hunk: Hunk, side: PatchSide, result: number[]) {
  let additionLine = hunk.additionStart;
  let deletionLine = hunk.deletionStart;
  for (const block of hunk.hunkContent) {
    const isContext = block.type === "context";
    const count = blockSideCount(block, side);
    const first = side === "additions" ? additionLine : deletionLine;
    for (let i = 0; i < count; i++) {
      result.push(first + i);
    }
    additionLine += isContext ? block.lines : block.additions;
    deletionLine += isContext ? block.lines : block.deletions;
  }
}

function blockSideCount(
  block: Hunk["hunkContent"][number],
  side: PatchSide
): number {
  if (block.type === "context") {
    return block.lines;
  }
  return side === "additions" ? block.additions : block.deletions;
}
