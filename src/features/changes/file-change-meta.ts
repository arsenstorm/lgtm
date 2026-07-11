import type { FileDiffMetadata } from "@pierre/diffs/react";

export type ChangeGlyph = {
  letter: string;
  label: string;
  className: string;
};

const CHANGE_GLYPHS: Record<FileDiffMetadata["type"], ChangeGlyph> = {
  new: {
    letter: "A",
    label: "Added",
    className: "text-emerald-600 dark:text-emerald-400",
  },
  deleted: {
    letter: "D",
    label: "Deleted",
    className: "text-red-600 dark:text-red-400",
  },
  change: {
    letter: "M",
    label: "Modified",
    className: "text-amber-600 dark:text-amber-400",
  },
  "rename-pure": {
    letter: "R",
    label: "Renamed",
    className: "text-sky-600 dark:text-sky-400",
  },
  "rename-changed": {
    letter: "R",
    label: "Renamed",
    className: "text-sky-600 dark:text-sky-400",
  },
};

export function changeGlyph(type: FileDiffMetadata["type"]): ChangeGlyph {
  return CHANGE_GLYPHS[type];
}

export type FileStats = { additions: number; deletions: number };

export function fileStats(file: FileDiffMetadata): FileStats {
  let additions = 0;
  let deletions = 0;
  for (const hunk of file.hunks) {
    additions += hunk.additionLines;
    deletions += hunk.deletionLines;
  }
  return { additions, deletions };
}

/** Files with no hunks are binary / submodule placeholders and can't be shown. */
export function isDisplayable(file: FileDiffMetadata): boolean {
  return file.hunks.length > 0;
}

/** Splits a path into its directory (dimmed in the UI) and base name. */
export function splitPath(path: string): { dir: string; name: string } {
  const index = path.lastIndexOf("/");
  if (index === -1) {
    return { dir: "", name: path };
  }
  return { dir: path.slice(0, index + 1), name: path.slice(index + 1) };
}
