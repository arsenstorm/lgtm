import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import type { DiffData } from "@/features/changes/use-diff";
import { type AppError, toAppError } from "@/lib/errors/app-error";
import { threadInlineComments } from "@/lib/github/inline-comment-map";
import {
  addConversationComment,
  deleteReviewComment,
  dismissReview,
  getPrCiStatus,
  listConversationComments,
  listPrInlineComments,
  listReviews,
  mergePullRequest,
  setPullRequestState,
} from "@/lib/tauri/github";
import type {
  ConversationComment,
  MergeMethod,
  PrCiStatus,
  PrInlineComment,
  ReviewInfo,
} from "@/types/github";

export type MergeArgs = {
  expectedHeadSha: string;
  method: MergeMethod;
  commitTitle: string | null;
  commitMessage: string | null;
  deleteBranch: boolean;
};

export type PrBusy = {
  merge: boolean;
  state: boolean;
  dismiss: boolean;
  deleteComment: boolean;
  comment: boolean;
};

const IDLE_BUSY: PrBusy = {
  merge: false,
  state: false,
  dismiss: false,
  deleteComment: false,
  comment: false,
};

export type PrLiveState = {
  loading: boolean;
  ciStatus: PrCiStatus | null;
  reviews: ReviewInfo[];
  /** Top-level GitHub inline comments keyed to their replies, creation order. */
  inlineThreads: Map<number, PrInlineComment[]>;
  conversation: ConversationComment[];
  busy: PrBusy;
  reload: () => Promise<void>;
  /** Resolves to null on success, or the AppError to surface inline (GitHub's
   * merge-blocked reason is authoritative). A moved head is handled internally. */
  merge: (args: MergeArgs) => Promise<AppError | null>;
  setState: (next: "open" | "closed") => Promise<boolean>;
  dismiss: (reviewId: number, message: string) => Promise<boolean>;
  deleteInlineComment: (commentId: number) => Promise<boolean>;
  addComment: (body: string) => Promise<boolean>;
};

type Snapshot = {
  ciStatus: PrCiStatus | null;
  reviews: ReviewInfo[];
  inlineThreads: Map<number, PrInlineComment[]>;
  conversation: ConversationComment[];
};

const EMPTY: Snapshot = {
  ciStatus: null,
  reviews: [],
  inlineThreads: new Map(),
  conversation: [],
};

/**
 * Owns the pull request's live GitHub state for the workspace: CI status, past
 * reviews, existing inline-comment threads, and the conversation, plus every
 * mutation (merge, close/reopen, dismiss, delete comment, add comment). Fetches
 * once per diff fetch (guarded on the DiffData object identity, the same guard
 * useReviewComments/useSuggestions use) and after every successful mutation.
 */
export function usePrState(args: {
  owner: string;
  repository: string;
  pullNumber: number;
  diffData: DiffData | null;
  /** Called after merge/close/reopen so the workspace can refresh the bundle. */
  onMutated?: () => void;
  /** Called when a merge fails because the head moved (refresh the diff). */
  onRevisionChanged?: () => void;
}): PrLiveState {
  const {
    owner,
    repository,
    pullNumber,
    diffData,
    onMutated,
    onRevisionChanged,
  } = args;
  const [snapshot, setSnapshot] = useState<Snapshot>(EMPTY);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<PrBusy>(IDLE_BUSY);
  const aliveRef = useRef(true);

  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [ciStatus, reviews, inline, conversation] = await Promise.all([
        getPrCiStatus(owner, repository, pullNumber),
        listReviews(owner, repository, pullNumber),
        listPrInlineComments(owner, repository, pullNumber),
        listConversationComments(owner, repository, pullNumber),
      ]);
      if (!aliveRef.current) {
        return;
      }
      setSnapshot({
        ciStatus,
        reviews,
        inlineThreads: threadInlineComments(inline),
        conversation,
      });
    } catch (error) {
      if (aliveRef.current) {
        toast.error("Could not load pull request state", {
          description: toAppError(error).message,
        });
      }
    } finally {
      if (aliveRef.current) {
        setLoading(false);
      }
    }
  }, [owner, repository, pullNumber]);

  // Fetch once the diff is parsed, and again on each refresh (new identity).
  useEffect(() => {
    if (diffData) {
      load();
    }
  }, [load, diffData]);

  const merge = useCallback(
    async (mergeArgs: MergeArgs): Promise<AppError | null> => {
      setBusy((prev) => ({ ...prev, merge: true }));
      try {
        const result = await mergePullRequest({
          owner,
          repository,
          pullNumber,
          ...mergeArgs,
        });
        const branchNote = result.branchDeleted ? " · branch deleted" : "";
        toast.success(result.message || "Merged", {
          description: `#${pullNumber}${branchNote}`,
        });
        onMutated?.();
        await load();
        return null;
      } catch (error) {
        const appError = toAppError(error);
        if (appError.code === "pull-request-revision-changed") {
          toast.error("The pull request changed", {
            description:
              "Its head moved since you loaded it. Refreshing so the diff re-anchors — review, then merge again.",
          });
          onRevisionChanged?.();
          return null;
        }
        return appError;
      } finally {
        setBusy((prev) => ({ ...prev, merge: false }));
      }
    },
    [owner, repository, pullNumber, load, onMutated, onRevisionChanged]
  );

  const setState = useCallback(
    async (next: "open" | "closed"): Promise<boolean> => {
      setBusy((prev) => ({ ...prev, state: true }));
      try {
        await setPullRequestState(owner, repository, pullNumber, next);
        toast.success(
          next === "closed" ? "Pull request closed" : "Pull request reopened"
        );
        onMutated?.();
        await load();
        return true;
      } catch (error) {
        toast.error("Could not update the pull request", {
          description: toAppError(error).message,
        });
        return false;
      } finally {
        setBusy((prev) => ({ ...prev, state: false }));
      }
    },
    [owner, repository, pullNumber, load, onMutated]
  );

  const dismiss = useCallback(
    async (reviewId: number, message: string): Promise<boolean> => {
      setBusy((prev) => ({ ...prev, dismiss: true }));
      try {
        await dismissReview(owner, repository, pullNumber, reviewId, message);
        toast.success("Review dismissed");
        await load();
        return true;
      } catch (error) {
        toast.error("Could not dismiss review", {
          description: toAppError(error).message,
        });
        return false;
      } finally {
        setBusy((prev) => ({ ...prev, dismiss: false }));
      }
    },
    [owner, repository, pullNumber, load]
  );

  const deleteInlineComment = useCallback(
    async (commentId: number): Promise<boolean> => {
      setBusy((prev) => ({ ...prev, deleteComment: true }));
      try {
        await deleteReviewComment(owner, repository, commentId);
        toast.success("Comment deleted");
        await load();
        return true;
      } catch (error) {
        toast.error("Could not delete comment", {
          description: toAppError(error).message,
        });
        return false;
      } finally {
        setBusy((prev) => ({ ...prev, deleteComment: false }));
      }
    },
    [owner, repository, load]
  );

  const addComment = useCallback(
    async (body: string): Promise<boolean> => {
      setBusy((prev) => ({ ...prev, comment: true }));
      try {
        await addConversationComment(owner, repository, pullNumber, body);
        await load();
        return true;
      } catch (error) {
        toast.error("Could not add comment", {
          description: toAppError(error).message,
        });
        return false;
      } finally {
        setBusy((prev) => ({ ...prev, comment: false }));
      }
    },
    [owner, repository, pullNumber, load]
  );

  return useMemo(
    () => ({
      loading,
      ciStatus: snapshot.ciStatus,
      reviews: snapshot.reviews,
      inlineThreads: snapshot.inlineThreads,
      conversation: snapshot.conversation,
      busy,
      reload: load,
      merge,
      setState,
      dismiss,
      deleteInlineComment,
      addComment,
    }),
    [
      loading,
      snapshot,
      busy,
      load,
      merge,
      setState,
      dismiss,
      deleteInlineComment,
      addComment,
    ]
  );
}
