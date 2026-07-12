import { describe, expect, it } from "vitest";
import type { RepositoryRecord } from "../../types/review";
import { GITHUB_PATH_PREFIX, resolveRepoIdentity } from "./repo-identity";

function record(overrides: Partial<RepositoryRecord>): RepositoryRecord {
  return {
    id: "id",
    path: "/repo",
    displayName: "repo",
    remoteUrl: null,
    defaultBaseBranch: null,
    lastOpenedAt: "2024-01-01T00:00:00.000Z",
    createdAt: "2024-01-01T00:00:00.000Z",
    ...overrides,
  };
}

describe("resolveRepoIdentity", () => {
  it("matches a local clone via an https remote", () => {
    const local = record({
      id: "local-1",
      path: "/Users/me/repo",
      remoteUrl: "https://github.com/octocat/hello-world.git",
    });

    const result = resolveRepoIdentity([local], "octocat", "hello-world");

    expect(result).toEqual({
      kind: "local",
      record: local,
      staleSynthetic: null,
    });
  });

  it("matches a local clone via a git@ remote, case-insensitively", () => {
    const local = record({
      id: "local-1",
      path: "/Users/me/repo",
      remoteUrl: "git@github.com:Octocat/Hello-World.git",
    });

    const result = resolveRepoIdentity([local], "octocat", "hello-world");

    expect(result).toEqual({
      kind: "local",
      record: local,
      staleSynthetic: null,
    });
  });

  it("returns the synthetic record when no local clone exists", () => {
    const synthetic = record({
      id: "synthetic-1",
      path: `${GITHUB_PATH_PREFIX}octocat/hello-world`,
      remoteUrl: "https://github.com/octocat/hello-world",
    });

    const result = resolveRepoIdentity([synthetic], "octocat", "hello-world");

    expect(result).toEqual({ kind: "synthetic", record: synthetic });
  });

  it("prefers the local record and reports the stale synthetic record when both exist", () => {
    const local = record({
      id: "local-1",
      path: "/Users/me/repo",
      remoteUrl: "https://github.com/octocat/hello-world.git",
    });
    const synthetic = record({
      id: "synthetic-1",
      path: `${GITHUB_PATH_PREFIX}octocat/hello-world`,
      remoteUrl: "https://github.com/octocat/hello-world",
    });

    const result = resolveRepoIdentity(
      [synthetic, local],
      "octocat",
      "hello-world"
    );

    expect(result).toEqual({
      kind: "local",
      record: local,
      staleSynthetic: synthetic,
    });
  });

  it("returns create when neither a local clone nor a synthetic record exists", () => {
    const result = resolveRepoIdentity([], "octocat", "hello-world");

    expect(result).toEqual({ kind: "create" });
  });

  it("ignores a github:// row for a different repository", () => {
    const otherSynthetic = record({
      id: "synthetic-2",
      path: `${GITHUB_PATH_PREFIX}octocat/other-repo`,
      remoteUrl: "https://github.com/octocat/other-repo",
    });

    const result = resolveRepoIdentity(
      [otherSynthetic],
      "octocat",
      "hello-world"
    );

    expect(result).toEqual({ kind: "create" });
  });

  it("ignores a local row whose remote isn't a GitHub remote", () => {
    const local = record({
      id: "local-1",
      path: "/Users/me/repo",
      remoteUrl: "https://gitlab.com/octocat/hello-world.git",
    });

    const result = resolveRepoIdentity([local], "octocat", "hello-world");

    expect(result).toEqual({ kind: "create" });
  });
});
