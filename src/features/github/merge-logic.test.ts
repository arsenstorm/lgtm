import { describe, expect, it } from "vitest";
import { mergeDisabledReason } from "./merge-logic";

describe("mergeDisabledReason", () => {
  const cases: Array<{
    name: string;
    args: Parameters<typeof mergeDisabledReason>[0];
    blocked: string | null;
    warning: string | null;
  }> = [
    {
      name: "clean open PR is enabled",
      args: { draft: false, state: "open", mergeable: true, failingChecks: 0 },
      blocked: null,
      warning: null,
    },
    {
      name: "unknown mergeability (null) does not block",
      args: { draft: false, state: "open", mergeable: null, failingChecks: 0 },
      blocked: null,
      warning: null,
    },
    {
      name: "draft blocks",
      args: { draft: true, state: "open", mergeable: true, failingChecks: 0 },
      blocked: "Draft PR",
      warning: null,
    },
    {
      name: "conflicts block",
      args: { draft: false, state: "open", mergeable: false, failingChecks: 0 },
      blocked: "Conflicts with base",
      warning: null,
    },
    {
      name: "closed blocks",
      args: {
        draft: false,
        state: "closed",
        mergeable: true,
        failingChecks: 0,
      },
      blocked: "Pull request is closed",
      warning: null,
    },
    {
      name: "merged blocks",
      args: {
        draft: false,
        state: "merged",
        mergeable: true,
        failingChecks: 0,
      },
      blocked: "Already merged",
      warning: null,
    },
    {
      name: "one failing check warns but does not block",
      args: { draft: false, state: "open", mergeable: true, failingChecks: 1 },
      blocked: null,
      warning: "1 check failing — merge anyway?",
    },
    {
      name: "multiple failing checks pluralize",
      args: { draft: false, state: "open", mergeable: true, failingChecks: 3 },
      blocked: null,
      warning: "3 checks failing — merge anyway?",
    },
    {
      name: "a hard block wins over a failing-check warning",
      args: { draft: true, state: "open", mergeable: true, failingChecks: 3 },
      blocked: "Draft PR",
      warning: null,
    },
  ];

  for (const { name, args, blocked, warning } of cases) {
    it(name, () => {
      expect(mergeDisabledReason(args)).toEqual({ blocked, warning });
    });
  }
});
