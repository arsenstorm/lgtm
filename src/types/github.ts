export type PullRequestInfo = {
  owner: string;
  repository: string;
  pullNumber: number;
  title: string;
  authorLogin: string;
  state: string;
  draft: boolean;
  baseRef: string;
  baseSha: string;
  headRef: string;
  headSha: string;
  changedFiles: number;
  additions: number;
  deletions: number;
  htmlUrl: string;
  viewerLogin: string;
};

export type GithubPrBundle = { info: PullRequestInfo; patch: string };

export type DeviceFlowStart = {
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
};

export type GithubSide = "LEFT" | "RIGHT";

export type GithubReviewCommentDraft = {
  path: string;
  body: string;
  line: number;
  side: GithubSide;
  startLine?: number;
  startSide?: GithubSide;
};

export type GithubReviewEvent = "COMMENT" | "APPROVE" | "REQUEST_CHANGES";

export type SubmittedReview = { reviewId: number; htmlUrl: string };

export type ImportedGithubComment = {
  id: string;
  pullNumber: number;
  path: string;
  body: string;
  diffHunk: string;
  originalLine: number | null;
  side: string | null;
  authorLogin: string;
  commentedAt: string;
};

export type ImportPage = {
  comments: ImportedGithubComment[];
  hasMore: boolean;
};

export type PullRequestSummary = {
  number: number;
  title: string;
  authorLogin: string;
  baseRef: string;
  headRef: string;
  draft: boolean;
  updatedAt: string;
  htmlUrl: string;
};

export type MergeMethod = "merge" | "squash" | "rebase";
export type MergeResult = {
  merged: boolean;
  sha: string | null;
  message: string;
  branchDeleted: boolean;
};
export type ReviewInfo = {
  id: number;
  authorLogin: string;
  state: string;
  body: string;
  submittedAt: string | null;
  htmlUrl: string;
};
export type PrInlineComment = {
  id: number;
  authorLogin: string;
  path: string;
  line: number | null;
  originalLine: number | null;
  side: string | null;
  body: string;
  createdAt: string;
  htmlUrl: string;
  inReplyToId: number | null;
};
export type ConversationComment = {
  id: number;
  authorLogin: string;
  body: string;
  createdAt: string;
  htmlUrl: string;
};
export type CheckRunInfo = {
  name: string;
  status: string;
  conclusion: string | null;
  detailsUrl: string | null;
};
export type PrCiStatus = {
  checkRuns: CheckRunInfo[];
  commitState: string;
  mergeable: boolean | null;
  mergeableState: string | null;
  headSha: string;
};
