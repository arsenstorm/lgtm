import type { ImportedGithubComment } from "../../types/github";
import { getDb } from "./database";

export async function insertImportedComments(
  repositoryId: string,
  comments: ImportedGithubComment[]
): Promise<ImportedGithubComment[]> {
  const db = await getDb();
  const now = new Date().toISOString();
  const inserted: ImportedGithubComment[] = [];
  for (const comment of comments) {
    const result = await db.execute(
      `INSERT OR IGNORE INTO imported_github_comments
         (id, repository_id, pull_number, path, body, diff_hunk, original_line, side, author_login, commented_at, imported_at)
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)`,
      [
        comment.id,
        repositoryId,
        comment.pullNumber,
        comment.path,
        comment.body,
        comment.diffHunk,
        comment.originalLine,
        comment.side,
        comment.authorLogin,
        comment.commentedAt,
        now,
      ]
    );
    if (result.rowsAffected > 0) {
      inserted.push(comment);
    }
  }
  return inserted;
}

export async function countImportedComments(
  repositoryId: string
): Promise<number> {
  const db = await getDb();
  const rows = await db.select<{ n: number }[]>(
    "SELECT COUNT(*) as n FROM imported_github_comments WHERE repository_id = $1",
    [repositoryId]
  );
  return rows[0]?.n ?? 0;
}
