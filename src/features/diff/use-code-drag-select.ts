import type { SelectedLineRange } from "@pierre/diffs/react";
import { type PointerEvent as ReactPointerEvent, useMemo, useRef } from "react";
import {
  lineRangeFromRows,
  pathTouchesGutter,
  rowFromEventPath,
} from "@/lib/diff/dom-selection";

/** Pointer travel (px, manhattan) before a press starts being a drag. */
const DRAG_THRESHOLD_PX = 4;

type DragState = {
  pointerId: number;
  anchorRow: HTMLElement;
  startX: number;
  startY: number;
  active: boolean;
  lastRange: SelectedLineRange | null;
};

function sameRange(a: SelectedLineRange, b: SelectedLineRange): boolean {
  return (
    a.start === b.start &&
    a.end === b.end &&
    a.side === b.side &&
    a.endSide === b.endSide
  );
}

/**
 * Line selection by dragging over the code itself, not just the gutter.
 * The diff lives in an open shadow root, so rows are resolved from each
 * event's `composedPath()`; native text selection is left alone (copying
 * still works) and gutter presses are ignored — the diff renderer owns those.
 * A plain click never selects — the drag arms only after real pointer
 * travel — and instead emits `null` to clear any existing selection.
 */
export function useCodeDragSelect(
  onSelectionChange: (range: SelectedLineRange | null) => void
) {
  const dragRef = useRef<DragState | null>(null);

  return useMemo(() => {
    const onPointerDown = (event: ReactPointerEvent) => {
      if (event.pointerType === "mouse" && event.button !== 0) {
        return;
      }
      const path = event.nativeEvent.composedPath();
      if (pathTouchesGutter(path)) {
        return;
      }
      const row = rowFromEventPath(path);
      if (!row) {
        return;
      }
      dragRef.current = {
        pointerId: event.pointerId,
        anchorRow: row,
        startX: event.clientX,
        startY: event.clientY,
        active: false,
        lastRange: null,
      };
    };

    const onPointerMove = (event: ReactPointerEvent) => {
      const drag = dragRef.current;
      if (!drag || event.pointerId !== drag.pointerId) {
        return;
      }
      const row = rowFromEventPath(event.nativeEvent.composedPath());
      if (!row) {
        return;
      }
      if (!drag.active) {
        const travel =
          Math.abs(event.clientX - drag.startX) +
          Math.abs(event.clientY - drag.startY);
        if (travel < DRAG_THRESHOLD_PX && row === drag.anchorRow) {
          return;
        }
        drag.active = true;
      }
      const range = lineRangeFromRows(drag.anchorRow, row);
      // A null range (e.g. the pointer crossed to the other side of a split
      // diff) keeps the last valid selection instead of clearing it.
      if (!range || (drag.lastRange && sameRange(drag.lastRange, range))) {
        return;
      }
      drag.lastRange = range;
      onSelectionChange(range);
    };

    const onPointerUp = (event: ReactPointerEvent) => {
      const drag = dragRef.current;
      if (drag?.pointerId !== event.pointerId) {
        return;
      }
      dragRef.current = null;
      if (!drag.active) {
        onSelectionChange(null);
      }
    };

    const onPointerCancel = (event: ReactPointerEvent) => {
      if (dragRef.current?.pointerId === event.pointerId) {
        dragRef.current = null;
      }
    };

    return {
      onPointerDown,
      onPointerMove,
      onPointerUp,
      onPointerCancel,
    };
  }, [onSelectionChange]);
}
