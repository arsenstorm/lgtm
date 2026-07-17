import { describe, expect, it } from "vitest";
import { patchCacheKeyPrefix } from "./use-diff";

describe("patchCacheKeyPrefix", () => {
  it("is stable for identical patches and distinct for different ones", () => {
    const patch = "diff --git a/a.ts b/a.ts\n+added line\n";
    expect(patchCacheKeyPrefix(patch)).toBe(patchCacheKeyPrefix(patch));
    expect(patchCacheKeyPrefix(patch)).not.toBe(
      patchCacheKeyPrefix(`${patch}-changed line\n`)
    );
  });
});
