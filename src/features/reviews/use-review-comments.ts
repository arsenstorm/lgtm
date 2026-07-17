import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import type { DiffData } from "@/features/changes/use-diff";
import { maybeCreateMemoryExample } from "@/features/memory/use-memory-collection";
import {
  createComment,
  deleteComment,
  listSessionComments,
  updateCommentAnchor,
  updateCommentBody,
  updateCommentStatus,
} from "@/lib/db/review-comments";
import { anchorSideToPatchSide } from "@/lib/diff/anchor";
import { reanchorAll } from "@/lib/diff/reanchor";
import { toAppError } from "@/lib/errors/app-error";
import { detectLanguage } from "@/lib/memory/language";
import type { DiffAnchor, ReviewComment } from "@/types/review";

export type CreateCommentOptions = {
  /** Skip seeding a memory example (used when accepting an existing suggestion). */
  skipMemoryExample?: boolean;
};

export type CreateComment = (
  anchor: DiffAnchor,
  body: string,
  options?: CreateCommentOptions
) => Promise<ReviewComment | null>;

export type FileCommentCount = { total: number; outdated: number };

/** True when two anchors point at the same code position (revisions aside). */
function samePosition(a: DiffAnchor, b: DiffAnchor): boolean {
  return (
    a.startLine === b.startLine &&
    a.endLine === b.endLine &&
    a.side === b.side &&
    a.contextHash === b.contextHash &&
    a.selectedCode === b.selectedCode
  );
}

/**
 * Re-anchors every draft comment against a freshly parsed diff. Outdated
 * comments are left frozen (never resurrected). Persists anchor moves and
 * outdated transitions, and returns the reconciled comment list.
 */
async function reconcile(
  loaded: ReviewComment[],
  diff: DiffData
): Promise<ReviewComment[]> {
  const filesByName = new Map(diff.files.map((file) => [file.name, file]));
  const baseRevision = diff.baseSha ?? "";
  const headRevision = diff.headSha ?? "";

  // Group re-anchorable (draft) comments by file; freeze the rest as-is.
  const drafts = new Map<string, ReviewComment[]>();
  const result: ReviewComment[] = [];
  for (const comment of loaded) {
    if (comment.status !== "draft") {
      result.push(comment);
      continue;
    }
    const bucket = drafts.get(comment.anchor.path);
    if (bucket) {
      bucket.push(comment);
    } else {
      drafts.set(comment.anchor.path, [comment]);
    }
  }

  const persists: Promise<void>[] = [];
  for (const [path, comments] of drafts) {
    const { anchored, outdated } = reanchorAll({
      comments,
      file: filesByName.get(path),
      baseRevision,
      headRevision,
    });
    for (const { comment, anchor } of anchored) {
      if (samePosition(comment.anchor, anchor)) {
        result.push(comment);
      } else {
        persists.push(updateCommentAnchor(comment.id, anchor));
        result.push({ ...comment, anchor });
      }
    }
    for (const comment of outdated) {
      persists.push(updateCommentStatus(comment.id, "outdated"));
      result.push({ ...comment, status: "outdated" });
    }
  }

  await Promise.allSettled(persists);
  return result;
}

/**
 * Loads a session's comments and keeps them re-anchored to the current diff.
 * The reconcile effect is keyed on the DiffData object identity (a fresh object
 * per fetch), so it runs once per successful diff fetch/refresh and not on
 * unrelated re-renders. The database stays the source of truth; local mutations
 * write through so a later refresh reloads a consistent view.
 */
export function useReviewComments(args: {
  sessionId: string | null;
  repositoryId: string;
  diffData: DiffData | null;
}) {
  const { sessionId, repositoryId, diffData } = args;
  const [comments, setComments] = useState<ReviewComment[]>([]);

  useEffect(() => {
    if (!sessionId) {
      setComments([]);
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const loaded = await listSessionComments(sessionId);
        if (cancelled) {
          return;
        }
        // Without a parsed diff yet there is nothing to re-anchor against;
        // show the stored comments and let the next fetch reconcile them.
        if (!diffData) {
          setComments(loaded);
          return;
        }
        const reconciled = await reconcile(loaded, diffData);
        if (!cancelled) {
          setComments(reconciled);
        }
      } catch (error) {
        if (!cancelled) {
          toast.error("Could not load review comments", {
            description: toAppError(error).message,
          });
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [sessionId, diffData]);

  const create = useCallback(
    async (
      anchor: DiffAnchor,
      body: string,
      options?: CreateCommentOptions
    ): Promise<ReviewComment | null> => {
      if (!sessionId) {
        return null;
      }
      try {
        const comment = await createComment({
          reviewSessionId: sessionId,
          anchor,
          body,
          language: detectLanguage(anchor.path),
        });
        setComments((prev) => [...prev, comment]);
        // Fire-and-forget: seeding a memory example must never fail the save.
        if (!options?.skipMemoryExample) {
          maybeCreateMemoryExample({ comment, repositoryId }).catch(() => {
            // Silent by design; collection is a background nicety.
          });
        }
        return comment;
      } catch (error) {
        toast.error("Could not save comment", {
          description: toAppError(error).message,
        });
        return null;
      }
    },
    [sessionId, repositoryId]
  );

  const edit = useCallback(async (id: string, body: string): Promise<void> => {
    try {
      await updateCommentBody(id, body);
      setComments((prev) =>
        prev.map((comment) =>
          comment.id === id
            ? { ...comment, body, updatedAt: new Date().toISOString() }
            : comment
        )
      );
    } catch (error) {
      toast.error("Could not update comment", {
        description: toAppError(error).message,
      });
    }
  }, []);

  const remove = useCallback(async (id: string): Promise<void> => {
    try {
      await deleteComment(id);
      setComments((prev) => prev.filter((comment) => comment.id !== id));
    } catch (error) {
      toast.error("Could not delete comment", {
        description: toAppError(error).message,
      });
    }
  }, []);

  const markPublished = useCallback(async (ids: string[]): Promise<void> => {
    // Persist first so a later refresh reloads them as published (not resent).
    await Promise.allSettled(
      ids.map((id) => updateCommentStatus(id, "published"))
    );
    const published = new Set(ids);
    setComments((prev) =>
      prev.map((comment) =>
        published.has(comment.id)
          ? {
              ...comment,
              status: "published",
              updatedAt: new Date().toISOString(),
            }
          : comment
      )
    );
  }, []);

  const ordered = useMemo(
    () =>
      [...comments].sort((a, b) => {
        if (a.anchor.path !== b.anchor.path) {
          return a.anchor.path < b.anchor.path ? -1 : 1;
        }
        return a.anchor.startLine - b.anchor.startLine;
      }),
    [comments]
  );

  const byFile = useMemo(() => {
    const map = new Map<string, ReviewComment[]>();
    for (const comment of ordered) {
      const bucket = map.get(comment.anchor.path);
      if (bucket) {
        bucket.push(comment);
      } else {
        map.set(comment.anchor.path, [comment]);
      }
    }
    return map;
  }, [ordered]);

  const counts = useMemo(() => {
    const map = new Map<string, FileCommentCount>();
    for (const comment of comments) {
      const current = map.get(comment.anchor.path) ?? { total: 0, outdated: 0 };
      current.total += 1;
      if (comment.status === "outdated") {
        current.outdated += 1;
      }
      map.set(comment.anchor.path, current);
    }
    return map;
  }, [comments]);

  return {
    comments,
    ordered,
    byFile,
    counts,
    create,
    edit,
    remove,
    markPublished,
  };
}

/** Maps an anchor's stored side to the patch side used for annotations. */
export function commentAnnotationSide(comment: ReviewComment) {
  return anchorSideToPatchSide(comment.anchor.side);
}
