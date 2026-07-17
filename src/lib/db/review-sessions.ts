import type { ReviewSession, ReviewSessionStatus } from "../../types/review";
import { getDb } from "./database";

interface ReviewSessionRow {
  base_revision: string | null;
  base_sha: string | null;
  created_at: string;
  head_revision: string | null;
  head_sha: string | null;
  id: string;
  pull_number: number | null;
  repository_id: string;
  source_kind: string;
  status: string;
  updated_at: string;
}

function rowToSession(row: ReviewSessionRow): ReviewSession {
  return {
    id: row.id,
    repositoryId: row.repository_id,
    sourceKind: row.source_kind as ReviewSession["sourceKind"],
    baseRevision: row.base_revision,
    headRevision: row.head_revision,
    baseSha: row.base_sha,
    headSha: row.head_sha,
    pullNumber: row.pull_number,
    status: row.status as ReviewSessionStatus,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

export async function getOrCreateOpenSession(args: {
  repositoryId: string;
  sourceKind: "working-tree" | "branch" | "github-pull-request";
  baseRevision: string | null;
  headRevision: string | null;
  pullNumber?: number | null;
}): Promise<ReviewSession> {
  const db = await getDb();
  const pullNumber = args.pullNumber ?? null;
  const existing = await db.select<ReviewSessionRow[]>(
    `SELECT * FROM review_sessions
     WHERE repository_id = $1 AND source_kind = $2 AND status = 'open' AND base_revision IS $3 AND pull_number IS $4
     LIMIT 1`,
    [args.repositoryId, args.sourceKind, args.baseRevision, pullNumber]
  );
  if (existing.length > 0) {
    return rowToSession(existing[0]);
  }

  const id = crypto.randomUUID();
  const now = new Date().toISOString();
  await db.execute(
    `INSERT INTO review_sessions
       (id, repository_id, source_kind, base_revision, head_revision, base_sha, head_sha, pull_number, status, created_at, updated_at)
     VALUES ($1, $2, $3, $4, $5, NULL, NULL, $6, 'open', $7, $7)`,
    [
      id,
      args.repositoryId,
      args.sourceKind,
      args.baseRevision,
      args.headRevision,
      pullNumber,
      now,
    ]
  );
  const created = await db.select<ReviewSessionRow[]>(
    "SELECT * FROM review_sessions WHERE id = $1",
    [id]
  );
  return rowToSession(created[0]);
}

export async function updateSessionShas(
  id: string,
  baseSha: string | null,
  headSha: string | null
): Promise<void> {
  const db = await getDb();
  await db.execute(
    "UPDATE review_sessions SET base_sha = $1, head_sha = $2, updated_at = $3 WHERE id = $4",
    [baseSha, headSha, new Date().toISOString(), id]
  );
}

export async function closeSession(id: string): Promise<void> {
  const db = await getDb();
  await db.execute(
    "UPDATE review_sessions SET status = 'closed', updated_at = $1 WHERE id = $2",
    [new Date().toISOString(), id]
  );
}

export async function getSession(id: string): Promise<ReviewSession | null> {
  const db = await getDb();
  const rows = await db.select<ReviewSessionRow[]>(
    "SELECT * FROM review_sessions WHERE id = $1",
    [id]
  );
  return rows[0] ? rowToSession(rows[0]) : null;
}
