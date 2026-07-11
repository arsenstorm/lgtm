import type {
  MemoryExample,
  MemoryFingerprint,
  MemoryScope,
} from "../../types/review";
import { getDb } from "./database";

interface MemoryExampleRow {
  comment_body: string;
  context_after: string;
  context_before: string;
  created_at: string;
  enabled: number;
  file_path: string;
  fingerprint: string;
  id: string;
  language: string | null;
  negative_feedback: number;
  normalized_code: string;
  positive_feedback: number;
  repository_id: string | null;
  scope: string;
  selected_code: string;
  source_comment_id: string | null;
  updated_at: string;
}

const EMPTY_FINGERPRINT: MemoryFingerprint = {
  trigrams: [],
  shape: [],
  identifiers: [],
  lineCount: 0,
};

function parseFingerprint(raw: string): MemoryFingerprint {
  try {
    const parsed: unknown = JSON.parse(raw);
    if (
      typeof parsed === "object" &&
      parsed !== null &&
      Array.isArray((parsed as MemoryFingerprint).trigrams) &&
      Array.isArray((parsed as MemoryFingerprint).shape) &&
      Array.isArray((parsed as MemoryFingerprint).identifiers) &&
      typeof (parsed as MemoryFingerprint).lineCount === "number"
    ) {
      return parsed as MemoryFingerprint;
    }
    return EMPTY_FINGERPRINT;
  } catch {
    return EMPTY_FINGERPRINT;
  }
}

function rowToExample(row: MemoryExampleRow): MemoryExample {
  return {
    id: row.id,
    sourceCommentId: row.source_comment_id,
    repositoryId: row.repository_id,
    scope: row.scope as MemoryScope,
    language: row.language,
    commentBody: row.comment_body,
    selectedCode: row.selected_code,
    contextBefore: row.context_before,
    contextAfter: row.context_after,
    filePath: row.file_path,
    normalizedCode: row.normalized_code,
    fingerprint: parseFingerprint(row.fingerprint),
    enabled: row.enabled === 1,
    positiveFeedback: row.positive_feedback,
    negativeFeedback: row.negative_feedback,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

export async function createMemoryExample(
  args: Omit<
    MemoryExample,
    | "id"
    | "createdAt"
    | "updatedAt"
    | "positiveFeedback"
    | "negativeFeedback"
    | "enabled"
  >
): Promise<MemoryExample> {
  const db = await getDb();
  const id = crypto.randomUUID();
  const now = new Date().toISOString();
  await db.execute(
    `INSERT INTO memory_examples
       (id, source_comment_id, repository_id, scope, language, comment_body, selected_code,
        context_before, context_after, file_path, normalized_code, fingerprint, enabled,
        positive_feedback, negative_feedback, created_at, updated_at)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, 1, 0, 0, $13, $13)`,
    [
      id,
      args.sourceCommentId,
      args.repositoryId,
      args.scope,
      args.language,
      args.commentBody,
      args.selectedCode,
      args.contextBefore,
      args.contextAfter,
      args.filePath,
      args.normalizedCode,
      JSON.stringify(args.fingerprint),
      now,
    ]
  );
  return {
    ...args,
    id,
    enabled: true,
    positiveFeedback: 0,
    negativeFeedback: 0,
    createdAt: now,
    updatedAt: now,
  };
}

export async function listEnabledExamples(args: {
  language: string | null;
  repositoryId: string | null;
}): Promise<MemoryExample[]> {
  const db = await getDb();
  const params: unknown[] = [];
  let languageClause: string;
  if (args.language === null) {
    languageClause = "language IS NULL";
  } else {
    params.push(args.language);
    languageClause = `language = $${params.length}`;
  }
  params.push(args.repositoryId);
  const scopeParamIndex = params.length;
  const rows = await db.select<MemoryExampleRow[]>(
    `SELECT * FROM memory_examples
     WHERE enabled = 1 AND ${languageClause} AND (scope = 'global' OR repository_id = $${scopeParamIndex})`,
    params
  );
  return rows.map(rowToExample);
}

export async function setExampleEnabled(
  id: string,
  enabled: boolean
): Promise<void> {
  const db = await getDb();
  await db.execute("UPDATE memory_examples SET enabled = $1 WHERE id = $2", [
    enabled ? 1 : 0,
    id,
  ]);
}

export async function recordFeedback(
  id: string,
  kind: "positive" | "negative"
): Promise<void> {
  const db = await getDb();
  const column =
    kind === "positive" ? "positive_feedback" : "negative_feedback";
  await db.execute(
    `UPDATE memory_examples SET ${column} = ${column} + 1, updated_at = $1 WHERE id = $2`,
    [new Date().toISOString(), id]
  );
}

export async function getExample(id: string): Promise<MemoryExample | null> {
  const db = await getDb();
  const rows = await db.select<MemoryExampleRow[]>(
    "SELECT * FROM memory_examples WHERE id = $1",
    [id]
  );
  return rows[0] ? rowToExample(rows[0]) : null;
}

export async function listAllExamples(): Promise<MemoryExample[]> {
  const db = await getDb();
  const rows = await db.select<MemoryExampleRow[]>(
    "SELECT * FROM memory_examples ORDER BY created_at DESC"
  );
  return rows.map(rowToExample);
}
