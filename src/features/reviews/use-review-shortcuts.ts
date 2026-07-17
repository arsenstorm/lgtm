import { useEffect, useRef } from "react";

export type ReviewAction = {
  id: string;
  label: string;
  /** Rendered key hint, split into chunks for <Kbd> pills, e.g. ["⌘", "K"]. */
  hint: string[];
  /** event.key to match; single letters are compared case-insensitively. */
  key: string;
  /** Requires the platform meta/ctrl modifier. */
  meta?: boolean;
  run: () => void;
  disabled?: boolean;
};

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  const tag = target.tagName;
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT" ||
    target.isContentEditable
  );
}

function matches(action: ReviewAction, event: KeyboardEvent): boolean {
  const hasMeta = event.metaKey || event.ctrlKey;
  if (action.meta) {
    return hasMeta && event.key.toLowerCase() === action.key.toLowerCase();
  }
  if (hasMeta || event.altKey) {
    return false;
  }
  return event.key === action.key || event.key.toLowerCase() === action.key;
}

/**
 * Global keydown dispatcher for the review shortcuts. Editable targets are
 * skipped so typing never triggers a shortcut; the composer's own Cmd/Ctrl+Enter
 * and Escape are handled locally and never reach here.
 */
export function useReviewShortcuts(actions: ReviewAction[]) {
  const actionsRef = useRef(actions);
  actionsRef.current = actions;

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (isEditableTarget(event.target)) {
        return;
      }
      for (const action of actionsRef.current) {
        if (!action.disabled && matches(action, event)) {
          event.preventDefault();
          action.run();
          return;
        }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);
}
