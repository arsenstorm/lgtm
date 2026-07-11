import { describe, expect, it } from "vitest";
import type { GithubReviewEvent } from "@/types/github";
import { canSubmit } from "./submit-review-logic";

describe("canSubmit", () => {
  const cases: Array<{
    event: GithubReviewEvent;
    body: string;
    drafts: number;
    expected: boolean;
  }> = [
    { event: "COMMENT", body: "", drafts: 0, expected: false },
    { event: "COMMENT", body: "   ", drafts: 0, expected: false },
    { event: "COMMENT", body: "looks good", drafts: 0, expected: true },
    { event: "COMMENT", body: "", drafts: 1, expected: true },
    { event: "REQUEST_CHANGES", body: "", drafts: 0, expected: false },
    { event: "REQUEST_CHANGES", body: "please fix", drafts: 0, expected: true },
    { event: "REQUEST_CHANGES", body: "", drafts: 2, expected: true },
    { event: "APPROVE", body: "", drafts: 0, expected: true },
    { event: "APPROVE", body: "", drafts: 3, expected: true },
  ];

  for (const { event, body, drafts, expected } of cases) {
    it(`${event} body=${JSON.stringify(body)} drafts=${drafts} → ${expected}`, () => {
      expect(canSubmit(event, body, drafts)).toBe(expected);
    });
  }
});
