import type { CheckRunInfo, PrCiStatus } from "@/types/github";

export type CheckSummary = {
  total: number;
  pending: number;
  failing: number;
  succeeded: number;
};

// GitHub conclusions that count as a failure for merge purposes. Everything
// else on a completed run (success, neutral, skipped) is treated as passing.
const FAILING_CONCLUSIONS = new Set([
  "failure",
  "timed_out",
  "cancelled",
  "action_required",
  "stale",
  "startup_failure",
]);

export function summarizeChecks(runs: CheckRunInfo[]): CheckSummary {
  let pending = 0;
  let failing = 0;
  let succeeded = 0;
  for (const run of runs) {
    if (run.status !== "completed") {
      pending += 1;
    } else if (run.conclusion && FAILING_CONCLUSIONS.has(run.conclusion)) {
      failing += 1;
    } else {
      succeeded += 1;
    }
  }
  return { total: runs.length, pending, failing, succeeded };
}

export type CiTone = "pending" | "failure" | "success" | "unknown";

/**
 * The single tone the header chip shows. "unknown" only when there is genuinely
 * nothing to report (no runs AND the combined commit state is unknown), which is
 * how a missing Checks permission degrades.
 */
export function ciTone(status: PrCiStatus | null): CiTone {
  if (!status) {
    return "unknown";
  }
  const { pending, failing, total } = summarizeChecks(status.checkRuns);
  if (total === 0 && status.commitState === "unknown") {
    return "unknown";
  }
  if (pending > 0) {
    return "pending";
  }
  if (failing > 0) {
    return "failure";
  }
  return "success";
}
