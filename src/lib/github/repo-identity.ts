import type { RepositoryRecord } from "../../types/review";
import {
  listAllRepositories,
  mergeRepositoryRecords,
  touchRepository,
  upsertRepository,
} from "../db/repositories";
import { parseGithubRemote } from "./remote";

export const GITHUB_PATH_PREFIX = "github://";

export type RepoIdentityResolution =
  | {
      kind: "local";
      record: RepositoryRecord;
      staleSynthetic: RepositoryRecord | null;
    }
  | { kind: "synthetic"; record: RepositoryRecord }
  | { kind: "create" };

/**
 * Finds the canonical repository record for a GitHub owner/repo. A local
 * clone (matched by its parsed remote) wins over a synthetic github://
 * record; if both exist the synthetic one is reported for merging.
 * Matching is case-insensitive (GitHub names are).
 */
export function resolveRepoIdentity(
  rows: RepositoryRecord[],
  owner: string,
  repository: string
): RepoIdentityResolution {
  const wantOwner = owner.toLowerCase();
  const wantRepo = repository.toLowerCase();
  const syntheticPath =
    `${GITHUB_PATH_PREFIX}${owner}/${repository}`.toLowerCase();

  let local: RepositoryRecord | null = null;
  let synthetic: RepositoryRecord | null = null;
  for (const row of rows) {
    if (row.path.toLowerCase() === syntheticPath) {
      synthetic = synthetic ?? row;
      continue;
    }
    if (row.path.startsWith(GITHUB_PATH_PREFIX)) {
      continue;
    }
    const remote = parseGithubRemote(row.remoteUrl);
    if (
      remote &&
      remote.owner.toLowerCase() === wantOwner &&
      remote.repository.toLowerCase() === wantRepo
    ) {
      local = local ?? row;
    }
  }

  if (local) {
    return { kind: "local", record: local, staleSynthetic: synthetic };
  }
  if (synthetic) {
    return { kind: "synthetic", record: synthetic };
  }
  return { kind: "create" };
}

/**
 * Resolves (or creates) the repository record that a PR open should scope
 * sessions/memory to, merging any stale synthetic record into a local clone
 * when one exists.
 */
export async function repositoryRecordForPr(
  owner: string,
  repository: string
): Promise<RepositoryRecord> {
  const rows = await listAllRepositories();
  const resolution = resolveRepoIdentity(rows, owner, repository);

  if (resolution.kind === "local") {
    if (resolution.staleSynthetic) {
      await mergeRepositoryRecords(
        resolution.staleSynthetic.id,
        resolution.record.id
      );
    }
    await touchRepository(resolution.record.id);
    return resolution.record;
  }

  if (resolution.kind === "synthetic") {
    await touchRepository(resolution.record.id);
    return resolution.record;
  }

  // Synthetic record so sessions/memory scope to the GitHub repo. Only
  // path/displayName/remoteUrl/defaultBaseBranch are persisted.
  return await upsertRepository({
    rootPath: `${GITHUB_PATH_PREFIX}${owner}/${repository}`,
    displayName: `${owner}/${repository}`,
    currentBranch: null,
    headSha: null,
    detached: false,
    unborn: false,
    remoteUrl: `https://github.com/${owner}/${repository}`,
    defaultBaseBranch: null,
    branches: [],
    remoteBranches: [],
  });
}
