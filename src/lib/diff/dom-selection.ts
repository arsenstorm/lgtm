import type { SelectedLineRange } from "@pierre/diffs/react";

type PatchSideName = "additions" | "deletions";

type LinePoint = { line: number; side: PatchSideName };

const ROW_SELECTOR = "[data-line][data-line-type]";
const GUTTER_SELECTOR = "[data-column-number]";

function firstElement(path: readonly EventTarget[]): Element | null {
  for (const target of path) {
    if (target instanceof Element) {
      return target;
    }
  }
  return null;
}

/**
 * Resolves the diff line row under a pointer event. The diff renders inside an
 * open shadow root, so the event's `composedPath()` is the only reliable way
 * to reach the row elements (document-level selection APIs can't see them in
 * WebKit).
 */
export function rowFromEventPath(
  path: readonly EventTarget[]
): HTMLElement | null {
  const element = firstElement(path);
  const row = element?.closest(ROW_SELECTOR);
  return row instanceof HTMLElement ? row : null;
}

/** True when the pointer event landed on a line-number gutter cell. */
export function pathTouchesGutter(path: readonly EventTarget[]): boolean {
  return firstElement(path)?.closest(GUTTER_SELECTOR) != null;
}

function pointFromRow(row: HTMLElement): LinePoint | null {
  const line = Number.parseInt(row.getAttribute("data-line") ?? "", 10);
  if (Number.isNaN(line)) {
    return null;
  }
  const code = row.closest("[data-code]");
  if (!code) {
    return null;
  }
  const lineType = row.getAttribute("data-line-type");
  if (lineType === "change-deletion") {
    return { line, side: "deletions" };
  }
  if (lineType === "change-addition") {
    return { line, side: "additions" };
  }
  return {
    line,
    side: code.hasAttribute("data-deletions") ? "deletions" : "additions",
  };
}

/**
 * Maps two diff line rows (drag anchor and current pointer row) to a line
 * range. Returns null when either row can't be resolved or the rows sit on
 * opposite sides of a split diff (those can't anchor a comment).
 */
export function lineRangeFromRows(
  startRow: HTMLElement,
  endRow: HTMLElement
): SelectedLineRange | null {
  const start = pointFromRow(startRow);
  const end = pointFromRow(endRow);
  if (!(start && end) || start.side !== end.side) {
    return null;
  }
  const [low, high] = start.line <= end.line ? [start, end] : [end, start];
  return {
    start: low.line,
    end: high.line,
    side: low.side,
    endSide: high.side,
  };
}
