import { toast } from "sonner";
import type { ReviewComment } from "@/types/review";
import { describeAnchorRange } from "./anchor-range";

export type ReviewMarkdownInput = {
  repoName: string;
  comparisonLabel: string;
  date: Date;
  comments: ReviewComment[];
};

function commentBlock(comment: ReviewComment): string {
  const heading = describeAnchorRange(
    comment.anchor.startLine,
    comment.anchor.endLine,
    comment.anchor.side
  );
  const outdated = comment.status === "outdated" ? " _(outdated)_" : "";
  const fence = comment.language ?? "";
  const body = comment.body.trim();
  return [
    `- **${heading}**${outdated}`,
    "",
    `  \`\`\`${fence}`,
    ...comment.anchor.selectedCode.split("\n").map((line) => `  ${line}`),
    "  ```",
    "",
    ...body.split("\n").map((line) => `  ${line}`),
  ].join("\n");
}

/**
 * Builds the Markdown export for a review: a header, then each file as a
 * section with its comments. Pure — takes an explicit date so it is testable.
 */
export function buildReviewMarkdown(input: ReviewMarkdownInput): string {
  const lines: string[] = [
    `# Review — ${input.repoName}`,
    "",
    `${input.comparisonLabel} · ${input.date.toISOString().slice(0, 10)}`,
  ];

  if (input.comments.length === 0) {
    lines.push("", "_No comments._");
    return `${lines.join("\n")}\n`;
  }

  const byFile = new Map<string, ReviewComment[]>();
  for (const comment of input.comments) {
    const bucket = byFile.get(comment.anchor.path);
    if (bucket) {
      bucket.push(comment);
    } else {
      byFile.set(comment.anchor.path, [comment]);
    }
  }

  for (const [path, fileComments] of byFile) {
    lines.push("", `### ${path}`, "");
    lines.push(fileComments.map(commentBlock).join("\n"));
  }

  return `${lines.join("\n")}\n`;
}

/** Builds the export and copies it to the clipboard, toasting the result. */
export async function copyReviewMarkdown(
  input: ReviewMarkdownInput
): Promise<void> {
  const markdown = buildReviewMarkdown(input);
  try {
    await navigator.clipboard.writeText(markdown);
    toast.success("Review copied as Markdown");
  } catch {
    toast.error("Could not copy to clipboard");
  }
}
