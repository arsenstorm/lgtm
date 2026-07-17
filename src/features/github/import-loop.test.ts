import { describe, expect, it, vi } from "vitest";
import type { ImportedGithubComment, ImportPage } from "@/types/github";
import { type ImportDeps, MAX_PAGES, runImport } from "./import-loop";

function comment(id: string): ImportedGithubComment {
  return {
    id,
    pullNumber: 1,
    path: "src/a.ts",
    body: "note",
    diffHunk: "@@ -1 +1 @@",
    originalLine: 1,
    side: "RIGHT",
    authorLogin: "me",
    commentedAt: "2026-01-01T00:00:00.000Z",
  };
}

function page(ids: string[], hasMore: boolean): ImportPage {
  return { comments: ids.map(comment), hasMore };
}

function makeDeps(overrides: Partial<ImportDeps> = {}): ImportDeps {
  return {
    fetchPage: vi.fn((p: number) => Promise.resolve(page([`c${p}`], true))),
    // Default: everything is fresh, no dedup.
    insert: vi.fn((comments) => Promise.resolve(comments)),
    derive: vi.fn(() => Promise.resolve(1)),
    isCancelled: () => false,
    ...overrides,
  };
}

describe("runImport", () => {
  it("advances pages until hasMore is false", async () => {
    const fetchPage = vi.fn((p: number) =>
      Promise.resolve(page([`c${p}`], p < 3))
    );
    const deps = makeDeps({ fetchPage });

    const outcome = await runImport(deps);

    expect(fetchPage).toHaveBeenCalledTimes(3);
    expect(outcome.pagesFetched).toBe(3);
    expect(outcome.imported).toBe(3);
    expect(outcome.derived).toBe(3);
    expect(outcome.cancelled).toBe(false);
    expect(outcome.cappedWithMore).toBe(false);
  });

  it("stops between pages when cancelled and finishes the in-flight page", async () => {
    let calls = 0;
    const fetchPage = vi.fn((p: number) => {
      calls += 1;
      return Promise.resolve(page([`c${p}`], true));
    });
    // Cancel becomes true after the first page completes.
    const isCancelled = () => calls >= 1;
    const deps = makeDeps({ fetchPage, isCancelled });

    const outcome = await runImport(deps);

    expect(fetchPage).toHaveBeenCalledTimes(1);
    expect(outcome.cancelled).toBe(true);
    expect(outcome.imported).toBe(1);
  });

  it("counts only freshly inserted comments (dedup)", async () => {
    const fetchPage = vi.fn(() => Promise.resolve(page(["a", "b"], false)));
    // Dedup drops everything.
    const insert = vi.fn(() => Promise.resolve([]));
    const derive = vi.fn(() => Promise.resolve(0));
    const deps = makeDeps({ fetchPage, insert, derive });

    const outcome = await runImport(deps);

    expect(outcome.imported).toBe(0);
    expect(outcome.derived).toBe(0);
    expect(derive).toHaveBeenCalledWith([]);
  });

  it("stops at the page cap and flags cappedWithMore", async () => {
    const fetchPage = vi.fn((p: number) =>
      Promise.resolve(page([`c${p}`], true))
    );
    const deps = makeDeps({ fetchPage });

    const outcome = await runImport(deps);

    expect(fetchPage).toHaveBeenCalledTimes(MAX_PAGES);
    expect(outcome.pagesFetched).toBe(MAX_PAGES);
    expect(outcome.cappedWithMore).toBe(true);
    expect(outcome.cancelled).toBe(false);
  });

  it("does not flag cappedWithMore when the last page has no more", async () => {
    const fetchPage = vi.fn((p: number) =>
      Promise.resolve(page([`c${p}`], p < MAX_PAGES))
    );
    const deps = makeDeps({ fetchPage });

    const outcome = await runImport(deps);

    expect(fetchPage).toHaveBeenCalledTimes(MAX_PAGES);
    expect(outcome.cappedWithMore).toBe(false);
  });
});
