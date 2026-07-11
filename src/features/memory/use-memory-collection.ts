import { useCallback, useEffect, useState } from "react";
import { createMemoryExample } from "@/lib/db/memory-examples";
import { getSetting, setSetting } from "@/lib/db/settings";
import { shouldCreateMemoryExample } from "@/lib/memory/engine";
import { detectLanguage } from "@/lib/memory/language";
import { lexicalNormalizer } from "@/lib/memory/normalize";
import { buildFingerprint } from "@/lib/memory/similarity";
import type { ReviewComment } from "@/types/review";

const MEMORY_COLLECTION_KEY = "memory-collection-enabled";

/** Whether new review comments seed memory examples. Defaults ON when unset. */
export async function isMemoryCollectionEnabled(): Promise<boolean> {
  const value = await getSetting(MEMORY_COLLECTION_KEY);
  return value !== "false";
}

/**
 * Seeds a memory example from a freshly saved comment, when collection is on and
 * the comment/code is substantive. Best-effort: never throws into the caller so a
 * failure here can't break the comment save.
 */
export async function maybeCreateMemoryExample(args: {
  comment: ReviewComment;
  repositoryId: string;
}): Promise<void> {
  const { comment, repositoryId } = args;
  const { anchor } = comment;
  if (!(await isMemoryCollectionEnabled())) {
    return;
  }
  if (
    !shouldCreateMemoryExample({
      body: comment.body,
      selectedCode: anchor.selectedCode,
      filePath: anchor.path,
    })
  ) {
    return;
  }
  const normalized = lexicalNormalizer.normalize(anchor.selectedCode);
  await createMemoryExample({
    sourceCommentId: comment.id,
    repositoryId,
    scope: "repository",
    language: detectLanguage(anchor.path),
    commentBody: comment.body,
    selectedCode: anchor.selectedCode,
    contextBefore: anchor.contextBefore,
    contextAfter: anchor.contextAfter,
    filePath: anchor.path,
    normalizedCode: normalized.tokens.join(" "),
    fingerprint: buildFingerprint(normalized),
  });
}

/** Reads and toggles the memory-collection setting for the header control. */
export function useMemoryCollection() {
  const [enabled, setEnabled] = useState(true);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const value = await isMemoryCollectionEnabled();
        if (!cancelled) {
          setEnabled(value);
        }
      } catch {
        // Fall back to the optimistic default; a later toggle re-persists it.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const toggle = useCallback(async (next: boolean) => {
    setEnabled(next);
    await setSetting(MEMORY_COLLECTION_KEY, next ? "true" : "false");
  }, []);

  return { enabled, toggle };
}
