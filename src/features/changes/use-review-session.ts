import { useEffect, useState } from "react";
import { toast } from "sonner";
import { getOrCreateOpenSession } from "@/lib/db/review-sessions";
import { toAppError } from "@/lib/errors/app-error";
import type { ReviewSession } from "@/types/review";
import type { ComparisonMode } from "./comparison";

/**
 * Resolves the open review session for a repository + comparison. Viewed-state
 * persistence hangs off the returned session id.
 */
export function useReviewSession(args: {
  repositoryId: string | null;
  mode: ComparisonMode;
  headRevision: string;
}) {
  const { repositoryId, mode, headRevision } = args;
  const [session, setSession] = useState<ReviewSession | null>(null);
  const baseRevision = mode.kind === "branch" ? mode.base : null;

  useEffect(() => {
    if (!repositoryId) {
      setSession(null);
      return;
    }
    let cancelled = false;
    setSession(null);
    getOrCreateOpenSession({
      repositoryId,
      sourceKind: mode.kind,
      baseRevision,
      headRevision,
    })
      .then((next) => {
        if (!cancelled) {
          setSession(next);
        }
      })
      .catch((error) => {
        if (!cancelled) {
          // Session stays null and the viewer still renders; only viewed-state
          // persistence is lost, so surface it as a transient toast.
          toast.error("Could not start a review session", {
            description: toAppError(error).message,
          });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [repositoryId, mode.kind, baseRevision, headRevision]);

  return session;
}
