import type {
  DiffAnchor,
  ReviewComment,
  ReviewCommentStatus,
} from "../../types/review";
import { getDb } from "./database";

interface ReviewCommentRow {
  base_revision: string | null;
  body: string;
  context_after: string;
  context_before: string;
  context_hash: string;
  created_at: string;
  end_line: number;
  file_path: string;
  head_revision: string | null;
  hunk_header: string;
  id: string;
  language: string | null;
  review_session_id: string;
  selected_code: string;
  side: string;
  start_line: number;
  status: string;
  updated_at: string;
}

function anchorToColumns(anchor: DiffAnchor) {
  return {
    file_path: anchor.path,
    side: anchor.side,
    start_line: anchor.startLine,
    end_line: anchor.endLine,
    selected_code: anchor.selectedCode,
    context_before: anchor.contextBefore,
    context_after: anchor.contextAfter,
    context_hash: anchor.contextHash,
    hunk_header: anchor.hunkHeader,
    base_revision: anchor.baseRevision,
    head_revision: anchor.headRevision,
  };
}

function rowToComment(row: ReviewCommentRow): ReviewComment {
  return {
    id: row.id,
    reviewSessionId: row.review_session_id,
    anchor: {
      path: row.file_path,
      side: row.side as DiffAnchor["side"],
      startLine: row.start_line,
      endLine: row.end_line,
      baseRevision: row.base_revision ?? "",
      headRevision: row.head_revision ?? "",
      hunkHeader: row.hunk_header,
      selectedCode: row.selected_code,
      contextBefore: row.context_before,
      contextAfter: row.context_after,
      contextHash: row.context_hash,
    },
    body: row.body,
    language: row.language,
    status: row.status as ReviewCommentStatus,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

export async function createComment(args: {
  reviewSessionId: string;
  anchor: DiffAnchor;
  body: string;
  language: string | null;
}): Promise<ReviewComment> {
  const db = await getDb();
  const id = crypto.randomUUID();
  const now = new Date().toISOString();
  const cols = anchorToColumns(args.anchor);
  await db.execute(
    `INSERT INTO review_comments
       (id, review_session_id, file_path, side, start_line, end_line, body, status, language,
        selected_code, context_before, context_after, context_hash, hunk_header,
        base_revision, head_revision, created_at, updated_at)
     VALUES ($1, $2, $3, $4, $5, $6, $7, 'draft', $8, $9, $10, $11, $12, $13, $14, $15, $16, $16)`,
    [
      id,
      args.reviewSessionId,
      cols.file_path,
      cols.side,
      cols.start_line,
      cols.end_line,
      args.body,
      args.language,
      cols.selected_code,
      cols.context_before,
      cols.context_after,
      cols.context_hash,
      cols.hunk_header,
      cols.base_revision,
      cols.head_revision,
      now,
    ]
  );
  return {
    id,
    reviewSessionId: args.reviewSessionId,
    anchor: args.anchor,
    body: args.body,
    language: args.language,
    status: "draft",
    createdAt: now,
    updatedAt: now,
  };
}

export async function updateCommentBody(
  id: string,
  body: string
): Promise<void> {
  const db = await getDb();
  await db.execute(
    "UPDATE review_comments SET body = $1, updated_at = $2 WHERE id = $3",
    [body, new Date().toISOString(), id]
  );
}

export async function updateCommentStatus(
  id: string,
  status: ReviewCommentStatus
): Promise<void> {
  const db = await getDb();
  await db.execute(
    "UPDATE review_comments SET status = $1, updated_at = $2 WHERE id = $3",
    [status, new Date().toISOString(), id]
  );
}

export async function updateCommentAnchor(
  id: string,
  anchor: DiffAnchor
): Promise<void> {
  const db = await getDb();
  const cols = anchorToColumns(anchor);
  await db.execute(
    `UPDATE review_comments SET
       file_path = $1, side = $2, start_line = $3, end_line = $4,
       selected_code = $5, context_before = $6, context_after = $7,
       context_hash = $8, hunk_header = $9, base_revision = $10, head_revision = $11,
       updated_at = $12
     WHERE id = $13`,
    [
      cols.file_path,
      cols.side,
      cols.start_line,
      cols.end_line,
      cols.selected_code,
      cols.context_before,
      cols.context_after,
      cols.context_hash,
      cols.hunk_header,
      cols.base_revision,
      cols.head_revision,
      new Date().toISOString(),
      id,
    ]
  );
}

export async function listSessionComments(
  reviewSessionId: string
): Promise<ReviewComment[]> {
  const db = await getDb();
  const rows = await db.select<ReviewCommentRow[]>(
    `SELECT * FROM review_comments
     WHERE review_session_id = $1 AND status != 'deleted'
     ORDER BY file_path, start_line`,
    [reviewSessionId]
  );
  return rows.map(rowToComment);
}

export async function deleteComment(id: string): Promise<void> {
  const db = await getDb();
  await db.execute(
    "UPDATE review_comments SET status = 'deleted', updated_at = $1 WHERE id = $2",
    [new Date().toISOString(), id]
  );
}
