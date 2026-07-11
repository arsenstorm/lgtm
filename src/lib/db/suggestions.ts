import type {
  DiffAnchor,
  SuggestedComment,
  SuggestionStatus,
} from "../../types/review";
import { getDb } from "./database";

const EXPLANATION_TEXT = "Similar to a comment you made previously";

interface SuggestionRow {
  adjusted_confidence: number;
  anchor_json: string;
  created_at: string;
  file_path: string;
  id: string;
  memory_example_id: string;
  proposed_body: string;
  review_session_id: string;
  similarity_score: number;
  status: string;
  updated_at: string;
}

function rowToSuggestion(row: SuggestionRow): SuggestedComment {
  return {
    id: row.id,
    reviewSessionId: row.review_session_id,
    anchor: JSON.parse(row.anchor_json) as DiffAnchor,
    memoryExampleId: row.memory_example_id,
    proposedBody: row.proposed_body,
    similarityScore: row.similarity_score,
    adjustedConfidence: row.adjusted_confidence,
    status: row.status as SuggestionStatus,
    explanation: EXPLANATION_TEXT,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

export async function createSuggestion(args: {
  reviewSessionId: string;
  memoryExampleId: string;
  anchor: DiffAnchor;
  proposedBody: string;
  similarityScore: number;
  adjustedConfidence: number;
  explanation: string;
}): Promise<SuggestedComment> {
  const db = await getDb();
  const id = crypto.randomUUID();
  const now = new Date().toISOString();
  const anchorJson = JSON.stringify(args.anchor);
  await db.execute(
    `INSERT INTO suggestions
       (id, review_session_id, memory_example_id, file_path, anchor_json, proposed_body,
        similarity_score, adjusted_confidence, status, created_at, updated_at)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'proposed', $9, $9)`,
    [
      id,
      args.reviewSessionId,
      args.memoryExampleId,
      args.anchor.path,
      anchorJson,
      args.proposedBody,
      args.similarityScore,
      args.adjustedConfidence,
      now,
    ]
  );
  return {
    id,
    reviewSessionId: args.reviewSessionId,
    anchor: args.anchor,
    memoryExampleId: args.memoryExampleId,
    proposedBody: args.proposedBody,
    similarityScore: args.similarityScore,
    adjustedConfidence: args.adjustedConfidence,
    status: "proposed",
    explanation: EXPLANATION_TEXT,
    createdAt: now,
    updatedAt: now,
  };
}

export async function listSessionSuggestions(
  reviewSessionId: string,
  status?: SuggestionStatus
): Promise<SuggestedComment[]> {
  const db = await getDb();
  const rows = status
    ? await db.select<SuggestionRow[]>(
        "SELECT * FROM suggestions WHERE review_session_id = $1 AND status = $2",
        [reviewSessionId, status]
      )
    : await db.select<SuggestionRow[]>(
        "SELECT * FROM suggestions WHERE review_session_id = $1",
        [reviewSessionId]
      );
  return rows.map(rowToSuggestion);
}

export async function updateSuggestionStatus(
  id: string,
  status: SuggestionStatus
): Promise<void> {
  const db = await getDb();
  await db.execute(
    "UPDATE suggestions SET status = $1, updated_at = $2 WHERE id = $3",
    [status, new Date().toISOString(), id]
  );
}

export async function hasSuggestionForExampleInSession(
  reviewSessionId: string,
  memoryExampleId: string
): Promise<boolean> {
  const db = await getDb();
  const rows = await db.select<SuggestionRow[]>(
    "SELECT id FROM suggestions WHERE review_session_id = $1 AND memory_example_id = $2 LIMIT 1",
    [reviewSessionId, memoryExampleId]
  );
  return rows.length > 0;
}
