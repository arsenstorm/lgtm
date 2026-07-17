export type RepositoryInfo = {
  rootPath: string;
  displayName: string;
  currentBranch: string | null;
  headSha: string | null;
  detached: boolean;
  unborn: boolean;
  remoteUrl: string | null;
  defaultBaseBranch: string | null;
  branches: string[];
  remoteBranches: string[];
};

export type DiffSourceArgs =
  | { kind: "working-tree" }
  | { kind: "branch"; base: string; head?: string };

export type DiffResult = {
  patch: string;
  baseSha: string | null;
  headSha: string | null;
  untracked: string[];
};
