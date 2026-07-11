export type DiffSource =
  | { kind: "working-tree"; repositoryPath: string; headRevision: string }
  | {
      kind: "branch";
      repositoryPath: string;
      baseRevision: string;
      headRevision: string;
    }
  | {
      kind: "github-pull-request";
      owner: string;
      repository: string;
      pullNumber: number;
      baseSha: string;
      headSha: string;
    };

export type DiffAnchorSide = "old" | "new";

export type DiffAnchor = {
  path: string;
  side: DiffAnchorSide;
  startLine: number;
  endLine: number;
  baseRevision: string;
  headRevision: string;
  hunkHeader: string;
  selectedCode: string;
  contextBefore: string;
  contextAfter: string;
  contextHash: string;
};

export type ReviewCommentStatus =
  | "draft"
  | "published"
  | "outdated"
  | "deleted";

export type ReviewComment = {
  id: string;
  reviewSessionId: string;
  anchor: DiffAnchor;
  body: string;
  language: string | null;
  status: ReviewCommentStatus;
  createdAt: string;
  updatedAt: string;
};

export type SuggestionStatus =
  | "proposed"
  | "accepted"
  | "accepted-after-edit"
  | "dismissed"
  | "suppressed";

export type SuggestedComment = {
  id: string;
  reviewSessionId: string;
  anchor: DiffAnchor;
  memoryExampleId: string;
  proposedBody: string;
  similarityScore: number;
  adjustedConfidence: number;
  status: SuggestionStatus;
  explanation: string;
  createdAt: string;
  updatedAt: string;
};

export type MemoryScope = "repository" | "global";

export type MemoryFingerprint = {
  trigrams: string[];
  shape: string[];
  identifiers: string[];
  lineCount: number;
};

export type MemoryExample = {
  id: string;
  sourceCommentId: string | null;
  repositoryId: string | null;
  scope: MemoryScope;
  language: string | null;
  commentBody: string;
  selectedCode: string;
  contextBefore: string;
  contextAfter: string;
  filePath: string;
  normalizedCode: string;
  fingerprint: MemoryFingerprint;
  enabled: boolean;
  positiveFeedback: number;
  negativeFeedback: number;
  createdAt: string;
  updatedAt: string;
};

export type ReviewSessionStatus = "open" | "closed";

export type ReviewSession = {
  id: string;
  repositoryId: string;
  sourceKind: DiffSource["kind"];
  baseRevision: string | null;
  headRevision: string | null;
  baseSha: string | null;
  headSha: string | null;
  pullNumber: number | null;
  status: ReviewSessionStatus;
  createdAt: string;
  updatedAt: string;
};

export type RepositoryRecord = {
  id: string;
  path: string;
  displayName: string;
  remoteUrl: string | null;
  defaultBaseBranch: string | null;
  lastOpenedAt: string;
  createdAt: string;
};

export type FileReviewState = {
  reviewSessionId: string;
  filePath: string;
  viewed: boolean;
  lastViewedAt: string | null;
};
