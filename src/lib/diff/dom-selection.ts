import type { SelectedLineRange } from "@pierre/diffs/react";

type PatchSideName = "additions" | "deletions";

type LinePoint = { line: number; side: PatchSideName };

function rowFromNode(node: Node | null): HTMLElement | null {
  if (!node) {
    return null;
  }
  const element = node instanceof Element ? node : node.parentElement;
  return element?.closest<HTMLElement>("[data-line][data-line-type]") ?? null;
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
 * Maps two DOM nodes (native text-selection endpoints) to a diff line range.
 * Returns null when either endpoint is outside a rendered diff line or the
 * endpoints sit on opposite sides of a split diff (those can't anchor).
 */
export function lineRangeFromNodes(
  startNode: Node,
  endNode: Node
): SelectedLineRange | null {
  const startRow = rowFromNode(startNode);
  const endRow = rowFromNode(endNode);
  if (!(startRow && endRow)) {
    return null;
  }
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

/**
 * Reads the window's current text selection and, when it lies inside
 * `container`, converts it to a diff line range.
 */
export function selectionToLineRange(
  container: HTMLElement
): SelectedLineRange | null {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) {
    return null;
  }
  const range = selection.getRangeAt(0);
  if (!container.contains(range.commonAncestorContainer)) {
    return null;
  }
  return lineRangeFromNodes(range.startContainer, range.endContainer);
}
