import type { FileDiffMetadata } from "@pierre/diffs/react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ChangedFileList } from "./changed-file-list";

afterEach(cleanup);

const NOT_SHOWN = /\(not shown\)/;
const NOTES = /notes\.txt/;

const NOTES_FILE = {
  name: "notes.txt",
  type: "new",
  hunks: [],
} as unknown as FileDiffMetadata;

function renderList(untracked: string[]) {
  render(
    <TooltipProvider>
      <ChangedFileList
        commentCounts={new Map()}
        files={[NOTES_FILE]}
        loading={false}
        onSelect={vi.fn()}
        onToggleViewed={vi.fn()}
        selectedFile={null}
        suggestionCounts={new Map()}
        untracked={untracked}
        viewed={new Set()}
      />
    </TooltipProvider>
  );
}

it("shows untracked files as regular rows with a U glyph", () => {
  renderList(["notes.txt"]);

  expect(screen.getByText("notes.txt")).toBeInTheDocument();
  const glyph = screen.getByTitle("Untracked");
  expect(glyph).toHaveTextContent("U");
  expect(screen.queryByText(NOT_SHOWN)).not.toBeInTheDocument();
});

it("lists only failed untracked files in the not-shown section", () => {
  renderList(["notes.txt", "huge.bin"]);

  expect(screen.getByText("huge.bin")).toBeInTheDocument();
  expect(screen.getByText(NOT_SHOWN)).toBeInTheDocument();
  expect(screen.getAllByText(NOTES)).toHaveLength(1);
});
