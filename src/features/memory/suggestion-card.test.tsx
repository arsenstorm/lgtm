import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import type { DiffAnchor, SuggestedComment } from "@/types/review";
import { SuggestionCard } from "./suggestion-card";

afterEach(cleanup);

const ACCEPT_EDIT_BUTTON = /Accept edit/;

const ANCHOR: DiffAnchor = {
  path: "src/foo.ts",
  side: "new",
  startLine: 10,
  endLine: 12,
  baseRevision: "base",
  headRevision: "head",
  hunkHeader: "@@ -1 +1 @@",
  selectedCode: "const x = 1;",
  contextBefore: "",
  contextAfter: "",
  contextHash: "hash",
};

function makeSuggestion(
  overrides: Partial<SuggestedComment> = {}
): SuggestedComment {
  return {
    id: "sug-1",
    reviewSessionId: "session-1",
    anchor: ANCHOR,
    memoryExampleId: "example-1",
    proposedBody: "Consider extracting this into a helper.",
    similarityScore: 0.9,
    adjustedConfidence: 0.9,
    status: "proposed",
    explanation: "Similar to a comment you made previously",
    createdAt: "2026-01-01T00:00:00.000Z",
    updatedAt: "2026-01-01T00:00:00.000Z",
    ...overrides,
  };
}

function renderCard(suggestion: SuggestedComment) {
  const handlers = {
    onAccept: vi.fn(),
    onEditAndAccept: vi.fn(),
    onDismiss: vi.fn(),
    onNeverAgain: vi.fn(),
    loadExample: vi.fn().mockResolvedValue(null),
  };
  render(<SuggestionCard suggestion={suggestion} {...handlers} />);
  return handlers;
}

it("accepts a suggestion when Accept is clicked", async () => {
  const suggestion = makeSuggestion();
  const handlers = renderCard(suggestion);

  await userEvent.click(screen.getByRole("button", { name: "Accept" }));

  expect(handlers.onAccept).toHaveBeenCalledWith(suggestion);
  expect(handlers.onEditAndAccept).not.toHaveBeenCalled();
});

it("dismisses a suggestion when Dismiss is clicked", async () => {
  const suggestion = makeSuggestion();
  const handlers = renderCard(suggestion);

  await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));

  expect(handlers.onDismiss).toHaveBeenCalledWith(suggestion);
});

it("edits and accepts with the edited body", async () => {
  const suggestion = makeSuggestion();
  const handlers = renderCard(suggestion);

  await userEvent.click(screen.getByRole("button", { name: "Edit" }));
  const textarea = screen.getByRole("textbox");
  await userEvent.clear(textarea);
  await userEvent.type(textarea, "Reworded suggestion");
  await userEvent.click(
    screen.getByRole("button", { name: ACCEPT_EDIT_BUTTON })
  );

  expect(handlers.onEditAndAccept).toHaveBeenCalledWith(
    suggestion,
    "Reworded suggestion"
  );
  expect(handlers.onAccept).not.toHaveBeenCalled();
});

it("labels a high-confidence match", () => {
  renderCard(makeSuggestion({ adjustedConfidence: 0.9 }));
  expect(screen.getByText("High match")).toBeInTheDocument();
});

it("labels a possible-confidence match", () => {
  renderCard(makeSuggestion({ adjustedConfidence: 0.75 }));
  expect(screen.getByText("Possible match")).toBeInTheDocument();
});
