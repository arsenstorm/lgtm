import type { RepositoryInfo } from "../../types/git";
import type { RepositoryRecord } from "../../types/review";
import { getDb } from "./database";

interface RepositoryRow {
  created_at: string;
  default_base_branch: string | null;
  display_name: string;
  id: string;
  last_opened_at: string;
  path: string;
  remote_url: string | null;
}

function rowToRepository(row: RepositoryRow): RepositoryRecord {
  return {
    id: row.id,
    path: row.path,
    displayName: row.display_name,
    remoteUrl: row.remote_url,
    defaultBaseBranch: row.default_base_branch,
    lastOpenedAt: row.last_opened_at,
    createdAt: row.created_at,
  };
}

export async function upsertRepository(
  info: RepositoryInfo
): Promise<RepositoryRecord> {
  const db = await getDb();
  const now = new Date().toISOString();
  await db.execute(
    `INSERT INTO repositories (id, path, display_name, remote_url, default_base_branch, last_opened_at, created_at)
     VALUES ($1, $2, $3, $4, $5, $6, $7)
     ON CONFLICT(path) DO UPDATE SET
       display_name = excluded.display_name,
       remote_url = excluded.remote_url,
       default_base_branch = excluded.default_base_branch,
       last_opened_at = excluded.last_opened_at`,
    [
      crypto.randomUUID(),
      info.rootPath,
      info.displayName,
      info.remoteUrl,
      info.defaultBaseBranch,
      now,
      now,
    ]
  );
  const rows = await db.select<RepositoryRow[]>(
    "SELECT * FROM repositories WHERE path = $1",
    [info.rootPath]
  );
  return rowToRepository(rows[0]);
}

export async function listRecentRepositories(
  limit = 10
): Promise<RepositoryRecord[]> {
  const db = await getDb();
  const rows = await db.select<RepositoryRow[]>(
    "SELECT * FROM repositories ORDER BY last_opened_at DESC LIMIT $1",
    [limit]
  );
  return rows.map(rowToRepository);
}

export async function touchRepository(id: string): Promise<void> {
  const db = await getDb();
  await db.execute(
    "UPDATE repositories SET last_opened_at = $1 WHERE id = $2",
    [new Date().toISOString(), id]
  );
}
