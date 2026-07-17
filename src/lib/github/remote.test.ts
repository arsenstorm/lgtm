import { describe, expect, it } from "vitest";
import { parseGithubRemote } from "./remote";

describe("parseGithubRemote", () => {
  it.each([
    ["https://github.com/foo/bar.git", { owner: "foo", repository: "bar" }],
    ["https://github.com/foo/bar", { owner: "foo", repository: "bar" }],
    ["https://www.github.com/foo/bar/", { owner: "foo", repository: "bar" }],
    ["git@github.com:foo/bar.git", { owner: "foo", repository: "bar" }],
    ["ssh://git@github.com/foo/bar.git", { owner: "foo", repository: "bar" }],
  ])("accepts %s", (remoteUrl, expected) => {
    expect(parseGithubRemote(remoteUrl)).toEqual(expected);
  });

  it.each([
    [null],
    [""],
    ["https://gitlab.com/foo/bar.git"],
    ["https://github.com/foo"],
    ["git@bitbucket.org:foo/bar.git"],
    ["https://github.com/foo/bar/baz"],
  ])("rejects %s", (remoteUrl) => {
    expect(parseGithubRemote(remoteUrl)).toBeNull();
  });
});
