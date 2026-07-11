import type { ImportedGithubComment, ImportPage } from "@/types/github";

/** Deliberate v1 bound: import at most the 1000 most recent comments. */
export const MAX_PAGES = 10;

export type ImportProgress = {
  page: number;
  imported: number;
  derived: number;
};

export type ImportDeps = {
  fetchPage: (page: number) => Promise<ImportPage>;
  insert: (
    comments: ImportedGithubComment[]
  ) => Promise<ImportedGithubComment[]>;
  derive: (fresh: ImportedGithubComment[]) => Promise<number>;
  /** Checked between pages; the in-flight page always finishes. */
  isCancelled: () => boolean;
  onProgress?: (progress: ImportProgress) => void;
};

export type ImportOutcome = {
  imported: number;
  derived: number;
  pagesFetched: number;
  cancelled: boolean;
  /** Hit the page cap while GitHub still reported more comments. */
  cappedWithMore: boolean;
};

/**
 * Drives the paged import: fetch → insert (dedup) → derive memory examples,
 * one page at a time. Cancellation is cooperative and checked between pages.
 * Pure over its injected dependencies so the paging rules are unit-testable.
 */
export async function runImport(deps: ImportDeps): Promise<ImportOutcome> {
  let page = 1;
  let imported = 0;
  let derived = 0;
  let pagesFetched = 0;
  let cancelled = false;
  let cappedWithMore = false;

  while (page <= MAX_PAGES) {
    if (deps.isCancelled()) {
      cancelled = true;
      break;
    }
    const result = await deps.fetchPage(page);
    pagesFetched = page;
    const fresh = await deps.insert(result.comments);
    derived += await deps.derive(fresh);
    imported += fresh.length;
    deps.onProgress?.({ page, imported, derived });
    if (!result.hasMore) {
      break;
    }
    if (page === MAX_PAGES) {
      cappedWithMore = true;
      break;
    }
    page += 1;
  }

  return { imported, derived, pagesFetched, cancelled, cappedWithMore };
}
