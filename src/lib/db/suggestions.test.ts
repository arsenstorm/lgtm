import { afterEach, describe, expect, it } from "vitest";
import { createFakeDb } from "../../test/fake-db";
import type { DiffAnchor } from "../../types/review";
import { setDbForTesting } from "./database";
import { listSessionSuggestions } from "./suggestions";

afterEach(() => {
  setDbForTesting(null);
});

const anchor: DiffAnchor = {
  path: "src/foo.ts",
  side: "new",
  startLine: 1,
  endLine: 2,
  baseRevision: "base-sha",
  headRevision: "head-sha",
  hunkHeader: "@@ -1,2 +1,2 @@",
  selectedCode: "const x = 1;",
  contextBefore: "",
  contextAfter: "",
  contextHash: "hash",
};

describe("listSessionSuggestions", () => {
  it("round-trips the anchor through anchor_json", async () => {
    const { db, enqueueSelect } = createFakeDb();
    setDbForTesting(db);

    enqueueSelect([
      {
        id: "suggestion-1",
        review_session_id: "session-1",
        memory_example_id: "example-1",
        file_path: "src/foo.ts",
        anchor_json: JSON.stringify(anchor),
        proposed_body: "consider using const",
        similarity_score: 0.9,
        adjusted_confidence: 0.8,
        status: "proposed",
        created_at: "2024-01-01T00:00:00.000Z",
        updated_at: "2024-01-01T00:00:00.000Z",
      },
    ]);

    const [suggestion] = await listSessionSuggestions("session-1");

    expect(suggestion.anchor).toEqual(anchor);
    expect(suggestion.explanation).toBe(
      "Similar to a comment you made previously"
    );
    expect(suggestion.status).toBe("proposed");
  });
});
