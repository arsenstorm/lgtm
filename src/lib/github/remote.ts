const GITHUB_HTTPS =
  /^https?:\/\/(www\.)?github\.com\/([^/]+)\/([^/]+?)(\.git)?\/?$/;
const GITHUB_SSH =
  /^(ssh:\/\/)?git@github\.com[:/]([^/]+)\/([^/]+?)(\.git)?\/?$/;

export type GithubRemote = { owner: string; repository: string };

/**
 * Parses a git remote URL into a GitHub owner/repository pair. Returns null
 * for non-GitHub remotes (which is common and not an error).
 */
export function parseGithubRemote(
  remoteUrl: string | null
): GithubRemote | null {
  if (!remoteUrl) {
    return null;
  }
  const trimmed = remoteUrl.trim();
  const match = GITHUB_HTTPS.exec(trimmed) ?? GITHUB_SSH.exec(trimmed);
  if (!match) {
    return null;
  }
  const owner = match[2];
  const repository = match[3];
  if (!(owner && repository) || owner === "." || owner === "..") {
    return null;
  }
  return { owner, repository };
}
