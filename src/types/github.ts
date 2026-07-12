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
