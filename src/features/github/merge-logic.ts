/**
 * Whether the merge button is blocked (hard, disables the button) or merely a
 * warning (failing checks — GitHub branch protection is the real enforcer, so
 * we demote to a "merge anyway" confirm instead of a hard block).
 *
 * Shaped as {blocked, warning} rather than a single string: a hard block and a
 * "merge anyway" nudge read very differently in the UI, and collapsing them
 * would lose that distinction. blocked !== null → disabled; warning !== null →
 * enabled but requires an extra confirmation.
 */
export type MergeGate = { blocked: string | null; warning: string | null };

export function mergeDisabledReason(args: {
  draft: boolean;
  state: string;
  mergeable: boolean | null;
  failingChecks: number;
}): MergeGate {
  const { draft, state, mergeable, failingChecks } = args;
  if (state !== "open") {
    return {
      blocked: state === "merged" ? "Already merged" : "Pull request is closed",
      warning: null,
    };
  }
  if (draft) {
    return { blocked: "Draft PR", warning: null };
  }
  if (mergeable === false) {
    return { blocked: "Conflicts with base", warning: null };
  }
  if (failingChecks > 0) {
    return {
      blocked: null,
      warning: `${failingChecks} check${failingChecks === 1 ? "" : "s"} failing — merge anyway?`,
    };
  }
  return { blocked: null, warning: null };
}
