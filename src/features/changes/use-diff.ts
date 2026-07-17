import { parsePatchFiles } from "@pierre/diffs";
import type { FileDiffMetadata } from "@pierre/diffs/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { updateSessionShas } from "@/lib/db/review-sessions";
import { type AppError, toAppError } from "@/lib/errors/app-error";
import { getDiff } from "@/lib/tauri/commands";
import { type ComparisonMode, comparisonToSource } from "./comparison";

export type DiffData = {
  files: FileDiffMetadata[];
  untracked: string[];
  baseSha: string | null;
  headSha: string | null;
};

/** The raw patch + metadata a fetcher yields, before parsing. */
export type RawDiff = {
  patch: string;
  untracked: string[];
  baseSha: string | null;
  headSha: string | null;
};

export type DiffFetcher = () => Promise<RawDiff>;

type DiffState = {
  data: DiffData | null;
  loading: boolean;
  refreshing: boolean;
  error: AppError | null;
};

const PARSE_ERROR: AppError = {
  code: "internal",
  message: "The diff could not be parsed",
  details: "parsePatchFiles returned an unexpected shape for this patch.",
};

const FNV_OFFSET_BASIS = 0x81_1c_9d_c5;
const FNV_PRIME = 0x01_00_01_93;

/**
 * FNV-1a content hash used as the parsePatchFiles cache-key prefix. The same
 * patch always yields the same per-file cacheKeys, so the worker pool's
 * highlight cache survives file switches and refreshes that change nothing.
 */
export function patchCacheKeyPrefix(patch: string): string {
  let hash = FNV_OFFSET_BASIS;
  for (let i = 0; i < patch.length; i++) {
    // biome-ignore lint/suspicious/noBitwiseOperators: FNV-1a is bitwise by definition.
    hash ^= patch.charCodeAt(i);
    hash = Math.imul(hash, FNV_PRIME);
  }
  // biome-ignore lint/suspicious/noBitwiseOperators: >>> 0 casts to unsigned before stringifying.
  return (hash >>> 0).toString(36);
}

/**
 * Fetches a raw patch via `fetcher`, parses it, and keeps the parsed diff +
 * session SHAs in sync. A monotonic fetch token guards against races: when the
 * fetcher identity changes mid-flight, any in-flight response for a previous
 * fetcher is discarded instead of overwriting fresher state. Session SHAs are
 * persisted in a separate effect so a late-resolving session still records them
 * without triggering a refetch. Callers memoize `fetcher` on their own inputs
 * (repo path + comparison, PR url, …) so a stable fetcher re-runs only when
 * those change.
 */
export function useParsedDiff(fetcher: DiffFetcher, sessionId: string | null) {
  const [state, setState] = useState<DiffState>({
    data: null,
    loading: false,
    refreshing: false,
    error: null,
  });
  const tokenRef = useRef(0);

  const run = useCallback(
    async (isRefresh: boolean) => {
      const token = ++tokenRef.current;
      setState((prev) => ({
        ...prev,
        loading: !isRefresh,
        refreshing: isRefresh,
        error: isRefresh ? prev.error : null,
      }));

      try {
        const result = await fetcher();
        let files: FileDiffMetadata[];
        try {
          files =
            parsePatchFiles(result.patch, patchCacheKeyPrefix(result.patch))[0]
              ?.files ?? [];
        } catch {
          throw PARSE_ERROR;
        }
        if (token !== tokenRef.current) {
          return;
        }
        setState({
          data: {
            files,
            untracked: result.untracked,
            baseSha: result.baseSha,
            headSha: result.headSha,
          },
          loading: false,
          refreshing: false,
          error: null,
        });
      } catch (error) {
        if (token !== tokenRef.current) {
          return;
        }
        setState((prev) => ({
          ...prev,
          loading: false,
          refreshing: false,
          error: toAppError(error),
        }));
      }
    },
    [fetcher]
  );

  useEffect(() => {
    run(false);
  }, [run]);

  const { data } = state;
  useEffect(() => {
    if (sessionId && data) {
      updateSessionShas(sessionId, data.baseSha, data.headSha).catch(() => {
        // Best-effort bookkeeping; a failure here doesn't affect the diff view.
      });
    }
  }, [sessionId, data]);

  const refresh = useCallback(() => run(true), [run]);

  return { ...state, refresh };
}

/** Local-repository diff: fetches + parses the git diff for a comparison. */
export function useDiff(args: {
  rootPath: string;
  mode: ComparisonMode;
  sessionId: string | null;
}) {
  const { rootPath, mode, sessionId } = args;
  const fetcher = useCallback<DiffFetcher>(async () => {
    const result = await getDiff(rootPath, comparisonToSource(mode));
    return {
      patch: result.patch,
      untracked: result.untracked,
      baseSha: result.baseSha,
      headSha: result.headSha,
    };
  }, [rootPath, mode]);
  return useParsedDiff(fetcher, sessionId);
}
