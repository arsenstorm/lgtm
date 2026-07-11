import type { FileReviewState } from "../../types/review";
import { getDb } from "./database";

interface FileReviewStateRow {
  file_path: string;
  last_viewed_at: string | null;
  review_session_id: string;
  viewed: number;
}

function rowToFileReviewState(row: FileReviewStateRow): FileReviewState {
  return {
    reviewSessionId: row.review_session_id,
    filePath: row.file_path,
    viewed: row.viewed === 1,
    lastViewedAt: row.last_viewed_at,
  };
}

export async function setFileViewed(
  reviewSessionId: string,
  filePath: string,
  viewed: boolean
): Promise<void> {
  const db = await getDb();
  const lastViewedAt = viewed ? new Date().toISOString() : null;
  await db.execute(
    `INSERT INTO file_review_state (review_session_id, file_path, viewed, last_viewed_at)
     VALUES ($1, $2, $3, $4)
     ON CONFLICT(review_session_id, file_path) DO UPDATE SET
       viewed = excluded.viewed,
       last_viewed_at = CASE WHEN excluded.viewed = 1 THEN excluded.last_viewed_at ELSE file_review_state.last_viewed_at END`,
    [reviewSessionId, filePath, viewed ? 1 : 0, lastViewedAt]
  );
}

export async function listFileReviewState(
  reviewSessionId: string
): Promise<FileReviewState[]> {
  const db = await getDb();
  const rows = await db.select<FileReviewStateRow[]>(
    "SELECT * FROM file_review_state WHERE review_session_id = $1",
    [reviewSessionId]
  );
  return rows.map(rowToFileReviewState);
}
