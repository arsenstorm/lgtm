import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { type DiffFetcher, useParsedDiff } from "@/features/changes/use-diff";
import {
  getOrCreateOpenSession,
  updateSessionShas,
} from "@/lib/db/review-sessions";
import { toAppError } from "@/lib/errors/app-error";
import { openGithubPr } from "@/lib/tauri/github";
import type { PullRequestInfo } from "@/types/github";
import type { ReviewSession } from "@/types/review";

/**
 * Resolves the open review session for a pull request. Mirrors
 * useReviewSession but scopes on `github-pull-request` + pull number and seeds
 * the head/base SHAs from the loaded bundle so re-anchoring has stable
 * revisions from the start.
 */
export function usePrSession(args: {
  repositoryId: string | null;
  info: PullRequestInfo;
}) {
  const { repositoryId, info } = args;
  const [session, setSession] = useState<ReviewSession | null>(null);
  const { pullNumber, baseRef, headRef, baseSha, headSha } = info;

  useEffect(() => {
    if (!repositoryId) {
      setSession(null);
      return;
    }
    let cancelled = false;
    setSession(null);
    getOrCreateOpenSession({
      repositoryId,
      sourceKind: "github-pull-request",
      baseRevision: baseRef,
      headRevision: headRef,
      pullNumber,
    })
      .then(async (next) => {
        await updateSessionShas(next.id, baseSha, headSha);
        if (!cancelled) {
          setSession(next);
        }
      })
      .catch((error) => {
        if (!cancelled) {
          toast.error("Could not start a review session", {
            description: toAppError(error).message,
          });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [repositoryId, pullNumber, baseRef, headRef, baseSha, headSha]);

  return session;
}

/**
 * Pull-request diff: parses the bundle patch through the shared diff pipeline.
 * A refresh re-fetches the PR (re-verifying the token) so the workspace picks
 * up a moved head; untracked files never apply to a PR.
 */
export function usePrDiff(args: {
  url: string;
  patch: string;
  baseSha: string;
  headSha: string;
  sessionId: string | null;
}) {
  const { url, patch, baseSha, headSha, sessionId } = args;
  // First call parses the already-loaded bundle (no network); every later call
  // — i.e. a refresh — re-fetches the PR so a moved head SHA flows through and
  // drives re-anchoring. Keyed by record id upstream, so the seed fires once.
  const seededRef = useRef(false);
  const fetcher = useCallback<DiffFetcher>(async () => {
    if (!seededRef.current) {
      seededRef.current = true;
      return { patch, untracked: [], baseSha, headSha };
    }
    const fresh = await openGithubPr(url);
    return {
      patch: fresh.patch,
      untracked: [],
      baseSha: fresh.info.baseSha,
      headSha: fresh.info.headSha,
    };
  }, [url, patch, baseSha, headSha]);

  return useParsedDiff(fetcher, sessionId);
}
