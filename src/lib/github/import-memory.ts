import type { ImportedGithubComment } from "../../types/github";
import { createMemoryExample } from "../db/memory-examples";
import { shouldCreateMemoryExample } from "../memory/engine";
import { detectLanguage } from "../memory/language";
import { lexicalNormalizer } from "../memory/normalize";
import { buildFingerprint } from "../memory/similarity";

const HUNK_HEADER = /^@@/;

/** Extracts the commented code from a GitHub diff_hunk: the added lines if any, else the context/deleted lines. */
export function diffHunkToSelectedCode(diffHunk: string): string {
  const lines = diffHunk.split("\n").filter((line) => !HUNK_HEADER.test(line));
  const added = lines.filter((l) => l.startsWith("+")).map((l) => l.slice(1));
  if (added.length > 0) {
    return added.join("\n");
  }
  return lines
    .filter((l) => l.startsWith(" ") || l.startsWith("-"))
    .map((l) => l.slice(1))
    .join("\n");
}

export async function deriveExamplesFromImports(
  repositoryId: string,
  imported: ImportedGithubComment[]
): Promise<number> {
  let created = 0;
  for (const comment of imported) {
    try {
      const selectedCode = diffHunkToSelectedCode(comment.diffHunk);
      const language = detectLanguage(comment.path);
      if (
        language === null ||
        !shouldCreateMemoryExample({
          body: comment.body,
          selectedCode,
          filePath: comment.path,
        })
      ) {
        continue;
      }
      const normalized = lexicalNormalizer.normalize(selectedCode);
      await createMemoryExample({
        sourceCommentId: null,
        repositoryId,
        scope: "repository",
        language,
        commentBody: comment.body,
        selectedCode,
        contextBefore: "",
        contextAfter: "",
        filePath: comment.path,
        normalizedCode: normalized.tokens.join(" "),
        fingerprint: buildFingerprint(normalized),
      });
      created++;
    } catch {
      // ponytail: skip and continue — one bad import row must not abort the batch.
    }
  }
  return created;
}
