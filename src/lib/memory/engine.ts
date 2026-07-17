import type { FileDiffMetadata } from "@pierre/diffs";
import type { DiffAnchor, MemoryExample } from "../../types/review";
import { buildAnchor } from "../diff/anchor";
import {
  type AdditionRun,
  collectAdditionRuns,
  windowsForExample,
} from "./candidates";
import {
  hasSubstantiveBody,
  hasSubstantiveCode,
  isExcludedPath,
  isImportBlock,
  isMinifiedCode,
} from "./filters";
import { detectLanguage } from "./language";
import { lexicalNormalizer } from "./normalize";
import {
  adjustedConfidence,
  MAX_SUGGESTIONS_PER_FILE,
  MAX_SUGGESTIONS_PER_REVIEW,
  SUGGESTION_THRESHOLD,
  scoreSimilarity,
} from "./similarity";

const MAX_NEGATIVE_FEEDBACK = 5;

/** True when a review comment is substantive enough to seed a memory example. */
export function shouldCreateMemoryExample(args: {
  body: string;
  selectedCode: string;
  filePath: string;
}): boolean {
  return (
    hasSubstantiveBody(args.body) &&
    hasSubstantiveCode(args.selectedCode) &&
    !isExcludedPath(args.filePath) &&
    !isMinifiedCode(args.selectedCode)
  );
}

export type SuggestionDraft = {
  anchor: DiffAnchor;
  memoryExampleId: string;
  proposedBody: string;
  similarityScore: number;
  adjustedConfidence: number;
  explanation: string;
};

type GenerateSuggestionsArgs = {
  files: FileDiffMetadata[];
  examples: MemoryExample[];
  repositoryId: string;
  /** Comment ids created in the current session — their examples are skipped. */
  currentSessionCommentIds: Set<string>;
  /** Example ids already suggested in this session (any status). */
  alreadySuggestedExampleIds: Set<string>;
  baseRevision: string;
  headRevision: string;
};

function isEligibleExample(
  example: MemoryExample,
  language: string,
  args: Pick<
    GenerateSuggestionsArgs,
    "repositoryId" | "currentSessionCommentIds" | "alreadySuggestedExampleIds"
  >
): boolean {
  if (!example.enabled) {
    return false;
  }
  if (example.language !== language) {
    return false;
  }
  const scopeMatches =
    example.scope === "global" || example.repositoryId === args.repositoryId;
  if (!scopeMatches) {
    return false;
  }
  if (
    example.sourceCommentId &&
    args.currentSessionCommentIds.has(example.sourceCommentId)
  ) {
    return false;
  }
  if (args.alreadySuggestedExampleIds.has(example.id)) {
    return false;
  }
  return example.negativeFeedback < MAX_NEGATIVE_FEEDBACK;
}

/** Best (highest-confidence) window match for a single (file, example) pair, if any clears the threshold. */
function bestDraftForExample(args: {
  file: FileDiffMetadata;
  example: MemoryExample;
  runs: AdditionRun[];
  repositoryId: string;
  baseRevision: string;
  headRevision: string;
}): SuggestionDraft | null {
  const { file, example, runs, repositoryId, baseRevision, headRevision } =
    args;
  const exampleIsImportBlock = isImportBlock(example.selectedCode);
  const exampleContextTokens = lexicalNormalizer.normalize(
    `${example.contextBefore}\n${example.contextAfter}`
  ).tokens;

  let best: SuggestionDraft | null = null;

  for (const run of runs) {
    const runCode = run.lines.join("\n");
    if (isImportBlock(runCode) && !exampleIsImportBlock) {
      continue;
    }
    const windows = windowsForExample(run, example.fingerprint.lineCount);
    for (const window of windows) {
      if (!hasSubstantiveCode(window.code)) {
        continue;
      }
      const anchor = buildAnchor({
        file,
        side: "additions",
        startLine: window.startLine,
        endLine: window.endLine,
        baseRevision,
        headRevision,
      });
      if (!anchor) {
        continue;
      }
      const candidate = lexicalNormalizer.normalize(window.code);
      const candidateContextTokens = lexicalNormalizer.normalize(
        `${anchor.contextBefore}\n${anchor.contextAfter}`
      ).tokens;
      const breakdown = scoreSimilarity({
        fingerprint: example.fingerprint,
        candidate,
        exampleContextTokens,
        candidateContextTokens,
        sameRepository: example.repositoryId === repositoryId,
      });
      const confidence = adjustedConfidence(
        breakdown.score,
        example.positiveFeedback,
        example.negativeFeedback
      );
      if (
        breakdown.score < SUGGESTION_THRESHOLD ||
        confidence < SUGGESTION_THRESHOLD
      ) {
        continue;
      }
      if (!best || confidence > best.adjustedConfidence) {
        best = {
          anchor,
          memoryExampleId: example.id,
          proposedBody: example.commentBody,
          similarityScore: breakdown.score,
          adjustedConfidence: confidence,
          explanation: "Similar to a comment you made previously",
        };
      }
    }
  }

  return best;
}

function compareByRank(a: SuggestionDraft, b: SuggestionDraft): number {
  if (a.adjustedConfidence !== b.adjustedConfidence) {
    return b.adjustedConfidence - a.adjustedConfidence;
  }
  if (a.anchor.path !== b.anchor.path) {
    return a.anchor.path.localeCompare(b.anchor.path);
  }
  return a.anchor.startLine - b.anchor.startLine;
}

function comparePathThenLine(a: SuggestionDraft, b: SuggestionDraft): number {
  if (a.anchor.path !== b.anchor.path) {
    return a.anchor.path.localeCompare(b.anchor.path);
  }
  return a.anchor.startLine - b.anchor.startLine;
}

function overlaps(a: SuggestionDraft, b: SuggestionDraft): boolean {
  return (
    a.anchor.path === b.anchor.path &&
    a.anchor.startLine <= b.anchor.endLine &&
    b.anchor.startLine <= a.anchor.endLine
  );
}

/** Dedupes overlapping drafts, then caps per file and overall. */
function finalizeDrafts(drafts: SuggestionDraft[]): SuggestionDraft[] {
  const ranked = [...drafts].sort(compareByRank);

  const deduped: SuggestionDraft[] = [];
  for (const draft of ranked) {
    if (!deduped.some((kept) => overlaps(kept, draft))) {
      deduped.push(draft);
    }
  }

  const perFileCounts = new Map<string, number>();
  const withinFileCap: SuggestionDraft[] = [];
  for (const draft of deduped) {
    const count = perFileCounts.get(draft.anchor.path) ?? 0;
    if (count >= MAX_SUGGESTIONS_PER_FILE) {
      continue;
    }
    perFileCounts.set(draft.anchor.path, count + 1);
    withinFileCap.push(draft);
  }

  return withinFileCap
    .slice(0, MAX_SUGGESTIONS_PER_REVIEW)
    .sort(comparePathThenLine);
}

/** Generates deduplicated, thresholded, capped comment suggestions for a diff. */
export function generateSuggestions(
  args: GenerateSuggestionsArgs
): SuggestionDraft[] {
  const drafts: SuggestionDraft[] = [];

  for (const file of args.files) {
    if (isExcludedPath(file.name)) {
      continue;
    }
    const language = detectLanguage(file.name);
    if (!language) {
      continue;
    }
    const runs = collectAdditionRuns(file);
    if (runs.length === 0) {
      continue;
    }
    const joinedAddedCode = runs.flatMap((run) => run.lines).join("\n");
    if (isMinifiedCode(joinedAddedCode)) {
      continue;
    }

    const eligibleExamples = args.examples.filter((example) =>
      isEligibleExample(example, language, args)
    );

    for (const example of eligibleExamples) {
      const best = bestDraftForExample({
        file,
        example,
        runs,
        repositoryId: args.repositoryId,
        baseRevision: args.baseRevision,
        headRevision: args.headRevision,
      });
      if (best) {
        drafts.push(best);
      }
    }
  }

  return finalizeDrafts(drafts);
}
