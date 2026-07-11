import type { FileDiffMetadata } from "@pierre/diffs";
import { stripEol } from "../diff/patch-lines";

export type AdditionRun = {
  /** One-based first line number of the run on the new side. */
  startLine: number;
  /** EOL-stripped added lines. */
  lines: string[];
};

/** Contiguous runs of added lines (change blocks), per file. */
export function collectAdditionRuns(file: FileDiffMetadata): AdditionRun[] {
  const runs: AdditionRun[] = [];
  for (const hunk of file.hunks) {
    let additionLine = hunk.additionStart;
    for (const block of hunk.hunkContent) {
      if (block.type === "context") {
        additionLine += block.lines;
      } else {
        if (block.additions > 0) {
          const lines = file.additionLines
            .slice(
              block.additionLineIndex,
              block.additionLineIndex + block.additions
            )
            .map(stripEol);
          runs.push({ startLine: additionLine, lines });
        }
        additionLine += block.additions;
      }
    }
  }
  return runs;
}

export type CandidateWindow = {
  startLine: number;
  endLine: number;
  code: string;
};

const WINDOW_STEP_DIVISOR = 2;
const MAX_WINDOWS_PER_RUN = 40;

/**
 * Windows of ~exampleLineCount lines carved from a run. Small runs (at least
 * half the example size) are offered whole; long runs slide.
 */
export function windowsForExample(
  run: AdditionRun,
  exampleLineCount: number
): CandidateWindow[] {
  const size = Math.max(1, exampleLineCount);
  if (run.lines.length < Math.max(1, Math.ceil(size / 2))) {
    return [];
  }
  if (run.lines.length <= size) {
    return [
      {
        startLine: run.startLine,
        endLine: run.startLine + run.lines.length - 1,
        code: run.lines.join("\n"),
      },
    ];
  }
  const step = Math.max(1, Math.floor(size / WINDOW_STEP_DIVISOR));
  const windows: CandidateWindow[] = [];
  for (
    let offset = 0;
    offset + size <= run.lines.length && windows.length < MAX_WINDOWS_PER_RUN;
    offset += step
  ) {
    windows.push({
      startLine: run.startLine + offset,
      endLine: run.startLine + offset + size - 1,
      code: run.lines.slice(offset, offset + size).join("\n"),
    });
  }
  return windows;
}
