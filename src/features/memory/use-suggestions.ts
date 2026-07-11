import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import type { DiffData } from "@/features/changes/use-diff";
import type { CreateComment } from "@/features/reviews/use-review-comments";
import {
  listAllExamples,
  recordFeedback,
  setExampleEnabled,
} from "@/lib/db/memory-examples";
import {
  createSuggestion,
  listSessionSuggestions,
  updateSuggestionStatus,
} from "@/lib/db/suggestions";
import { toAppError } from "@/lib/errors/app-error";
import { generateSuggestions } from "@/lib/memory/engine";
import type { ReviewComment, SuggestedComment } from "@/types/review";

type UseSuggestionsArgs = {
  sessionId: string | null;
  repositoryId: string;
  diffData: DiffData | null;
  /** Current session comments — read live so generation skips their examples. */
  comments: ReviewComment[];
  createComment: CreateComment;
};

/**
 * Generates deterministic reviewer-memory suggestions for the current diff and
 * owns their lifecycle. Generation runs once per diff fetch (keyed on the
 * DiffData object identity, the same guard useReviewComments uses); comments are
 * read through a ref so a new comment doesn't retrigger generation. Only
 * "proposed" suggestions are rendered inline; acting on one removes it locally.
 */
export function useSuggestions({
  sessionId,
  repositoryId,
  diffData,
  comments,
  createComment,
}: UseSuggestionsArgs) {
  const [suggestions, setSuggestions] = useState<SuggestedComment[]>([]);
  const commentsRef = useRef(comments);
  commentsRef.current = comments;

  useEffect(() => {
    if (!(sessionId && diffData)) {
      setSuggestions([]);
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const existing = await listSessionSuggestions(sessionId);
        const examples = await listAllExamples();
        if (cancelled) {
          return;
        }
        const proposed = existing.filter((s) => s.status === "proposed");
        const drafts = generateSuggestions({
          files: diffData.files,
          examples,
          repositoryId,
          currentSessionCommentIds: new Set(
            commentsRef.current.map((comment) => comment.id)
          ),
          alreadySuggestedExampleIds: new Set(
            existing.map((s) => s.memoryExampleId)
          ),
          baseRevision: diffData.baseSha ?? "",
          headRevision: diffData.headSha ?? "",
        });
        const created: SuggestedComment[] = [];
        for (const draft of drafts) {
          created.push(
            await createSuggestion({ ...draft, reviewSessionId: sessionId })
          );
        }
        if (!cancelled) {
          setSuggestions([...proposed, ...created]);
        }
      } catch (error) {
        if (!cancelled) {
          toast.error("Could not generate suggestions", {
            description: toAppError(error).message,
          });
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [sessionId, repositoryId, diffData]);

  const drop = useCallback((id: string) => {
    setSuggestions((prev) => prev.filter((s) => s.id !== id));
  }, []);

  const accept = useCallback(
    async (suggestion: SuggestedComment) => {
      try {
        await createComment(suggestion.anchor, suggestion.proposedBody, {
          skipMemoryExample: true,
        });
        await updateSuggestionStatus(suggestion.id, "accepted");
        await recordFeedback(suggestion.memoryExampleId, "positive");
        drop(suggestion.id);
      } catch (error) {
        toast.error("Could not accept suggestion", {
          description: toAppError(error).message,
        });
      }
    },
    [createComment, drop]
  );

  const editAndAccept = useCallback(
    async (suggestion: SuggestedComment, editedBody: string) => {
      try {
        await createComment(suggestion.anchor, editedBody, {
          skipMemoryExample: true,
        });
        await updateSuggestionStatus(suggestion.id, "accepted-after-edit");
        await recordFeedback(suggestion.memoryExampleId, "positive");
        drop(suggestion.id);
      } catch (error) {
        toast.error("Could not accept suggestion", {
          description: toAppError(error).message,
        });
      }
    },
    [createComment, drop]
  );

  const dismiss = useCallback(
    async (suggestion: SuggestedComment) => {
      try {
        await updateSuggestionStatus(suggestion.id, "dismissed");
        await recordFeedback(suggestion.memoryExampleId, "negative");
        drop(suggestion.id);
      } catch (error) {
        toast.error("Could not dismiss suggestion", {
          description: toAppError(error).message,
        });
      }
    },
    [drop]
  );

  const neverAgain = useCallback(
    async (suggestion: SuggestedComment) => {
      try {
        await setExampleEnabled(suggestion.memoryExampleId, false);
        await updateSuggestionStatus(suggestion.id, "suppressed");
        drop(suggestion.id);
      } catch (error) {
        toast.error("Could not disable this memory", {
          description: toAppError(error).message,
        });
      }
    },
    [drop]
  );

  const byFile = useMemo(() => {
    const map = new Map<string, SuggestedComment[]>();
    for (const suggestion of suggestions) {
      const bucket = map.get(suggestion.anchor.path);
      if (bucket) {
        bucket.push(suggestion);
      } else {
        map.set(suggestion.anchor.path, [suggestion]);
      }
    }
    return map;
  }, [suggestions]);

  const counts = useMemo(() => {
    const map = new Map<string, number>();
    for (const suggestion of suggestions) {
      map.set(
        suggestion.anchor.path,
        (map.get(suggestion.anchor.path) ?? 0) + 1
      );
    }
    return map;
  }, [suggestions]);

  return {
    suggestions,
    byFile,
    counts,
    total: suggestions.length,
    accept,
    editAndAccept,
    dismiss,
    neverAgain,
  };
}
