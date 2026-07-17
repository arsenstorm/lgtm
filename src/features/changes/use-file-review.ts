import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { listFileReviewState, setFileViewed } from "@/lib/db/file-review-state";
import { toAppError } from "@/lib/errors/app-error";

/**
 * Tracks per-file viewed state for a review session. Toggles are optimistic and
 * revert on persistence failure.
 */
export function useFileReview(sessionId: string | null) {
  const [viewed, setViewed] = useState<Set<string>>(new Set());

  useEffect(() => {
    if (!sessionId) {
      setViewed(new Set());
      return;
    }
    let cancelled = false;
    listFileReviewState(sessionId)
      .then((rows) => {
        if (!cancelled) {
          setViewed(
            new Set(rows.filter((row) => row.viewed).map((row) => row.filePath))
          );
        }
      })
      .catch(() => {
        // Non-fatal: start from an empty set if state can't be loaded.
        if (!cancelled) {
          setViewed(new Set());
        }
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId]);

  const toggle = useCallback(
    async (filePath: string) => {
      if (!sessionId) {
        return;
      }
      const next = !viewed.has(filePath);
      setViewed((prev) => {
        const updated = new Set(prev);
        if (next) {
          updated.add(filePath);
        } else {
          updated.delete(filePath);
        }
        return updated;
      });
      try {
        await setFileViewed(sessionId, filePath, next);
      } catch (error) {
        // Revert the optimistic update.
        setViewed((prev) => {
          const updated = new Set(prev);
          if (next) {
            updated.delete(filePath);
          } else {
            updated.add(filePath);
          }
          return updated;
        });
        toast.error("Could not update viewed state", {
          description: toAppError(error).message,
        });
      }
    },
    [sessionId, viewed]
  );

  return { viewed, toggle };
}
