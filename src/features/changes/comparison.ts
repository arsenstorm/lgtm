import type { DiffSourceArgs } from "@/types/git";

export type ComparisonMode =
  | { kind: "working-tree" }
  | { kind: "branch"; base: string; head?: string };

export function comparisonToSource(mode: ComparisonMode): DiffSourceArgs {
  if (mode.kind === "branch") {
    return { kind: "branch", base: mode.base, head: mode.head };
  }
  return { kind: "working-tree" };
}

export function describeComparison(mode: ComparisonMode): string {
  if (mode.kind === "branch") {
    return mode.head
      ? `Comparing ${mode.head} against ${mode.base}`
      : `Comparing against ${mode.base}`;
  }
  return "Uncommitted changes in the working tree";
}
