import { describe, expect, it } from "vitest";
import type { CheckRunInfo, PrCiStatus } from "@/types/github";
import { ciTone, summarizeChecks } from "./ci-status";

function run(status: string, conclusion: string | null): CheckRunInfo {
  return { name: "ci", status, conclusion, detailsUrl: null };
}

function ci(checkRuns: CheckRunInfo[], commitState: string): PrCiStatus {
  return {
    checkRuns,
    commitState,
    mergeable: null,
    mergeableState: null,
    headSha: "abc",
  };
}

describe("summarizeChecks", () => {
  it("buckets pending, failing, and passing runs", () => {
    const summary = summarizeChecks([
      run("in_progress", null),
      run("queued", null),
      run("completed", "failure"),
      run("completed", "timed_out"),
      run("completed", "success"),
      run("completed", "neutral"),
      run("completed", "skipped"),
    ]);
    expect(summary).toEqual({
      total: 7,
      pending: 2,
      failing: 2,
      succeeded: 3,
    });
  });

  it("is all zeroes for no runs", () => {
    expect(summarizeChecks([])).toEqual({
      total: 0,
      pending: 0,
      failing: 0,
      succeeded: 0,
    });
  });
});

describe("ciTone", () => {
  it("is unknown for null status", () => {
    expect(ciTone(null)).toBe("unknown");
  });

  it("is unknown when no runs and commit state unknown (missing permission)", () => {
    expect(ciTone(ci([], "unknown"))).toBe("unknown");
  });

  it("pending wins over failing", () => {
    expect(
      ciTone(
        ci([run("in_progress", null), run("completed", "failure")], "pending")
      )
    ).toBe("pending");
  });

  it("failure when a completed run failed and none pending", () => {
    expect(ciTone(ci([run("completed", "failure")], "failure"))).toBe(
      "failure"
    );
  });

  it("success when every run passed", () => {
    expect(ciTone(ci([run("completed", "success")], "success"))).toBe(
      "success"
    );
  });
});
