import { lexicalNormalizer } from "./normalize";

const MAX_REASONABLE_LINE_LENGTH = 500;
const MIN_SUBSTANTIVE_BODY_LENGTH = 20;
const MIN_SUBSTANTIVE_BODY_WORDS = 4;
const MIN_SUBSTANTIVE_CODE_TOKENS = 8;

const EXCLUDED_PATH_SEGMENTS = new Set([
  "node_modules",
  "vendor",
  "dist",
  "build",
  "target",
  "__generated__",
  "__snapshots__",
  ".next",
  "out",
]);

const EXCLUDED_BASENAMES = new Set([
  "package-lock.json",
  "yarn.lock",
  "bun.lock",
  "bun.lockb",
  "pnpm-lock.yaml",
  "Cargo.lock",
  "composer.lock",
  "Gemfile.lock",
  "go.sum",
  "poetry.lock",
  "uv.lock",
]);

const IMPORT_LINE_PATTERN =
  /^\s*(import\b|from\s+\S+\s+import\b|use\s+[A-Za-z_:]|require\s*\(|#include\b|using\s+[A-Za-z_])/;

const TRAILING_PUNCTUATION_PATTERN = /[.!?]+$/;
const WHITESPACE_PATTERN = /\s+/;

const GENERIC_COMMENTS = new Set([
  "nit",
  "why",
  "fix this",
  "fix",
  "todo",
  "lgtm",
  "ship it",
  "+1",
  "👍",
  "remove",
  "delete this",
  "typo",
  "formatting",
  "spacing",
  "same here",
  "same",
  "this too",
  "ditto",
]);

/** True when the path should never feed the memory engine (deps, build output, lockfiles, generated/minified/snapshot files). */
export function isExcludedPath(path: string): boolean {
  const segments = path.split("/").filter(Boolean);
  if (segments.some((segment) => EXCLUDED_PATH_SEGMENTS.has(segment))) {
    return true;
  }
  const basename = segments.at(-1) ?? path;
  if (EXCLUDED_BASENAMES.has(basename)) {
    return true;
  }
  return (
    basename.includes(".min.") ||
    basename.endsWith(".snap") ||
    basename.includes(".generated.")
  );
}

/** True when any single line of code is unreasonably long, a sign of minified output. */
export function isMinifiedCode(code: string): boolean {
  return code
    .split("\n")
    .some((line) => line.length > MAX_REASONABLE_LINE_LENGTH);
}

/** True when every non-empty line is an import/use/require/include statement. */
export function isImportBlock(code: string): boolean {
  const lines = code.split("\n").filter((line) => line.trim().length > 0);
  if (lines.length === 0) {
    return false;
  }
  return lines.every((line) => IMPORT_LINE_PATTERN.test(line));
}

/** True when a comment body is a low-signal boilerplate reaction. */
export function isGenericComment(body: string): boolean {
  const normalized = body
    .trim()
    .toLowerCase()
    .replace(TRAILING_PUNCTUATION_PATTERN, "");
  return GENERIC_COMMENTS.has(normalized);
}

/** True when a comment body carries enough content to be worth remembering. */
export function hasSubstantiveBody(body: string): boolean {
  const trimmed = body.trim();
  const words = trimmed.split(WHITESPACE_PATTERN).filter(Boolean);
  return (
    trimmed.length >= MIN_SUBSTANTIVE_BODY_LENGTH &&
    words.length >= MIN_SUBSTANTIVE_BODY_WORDS &&
    !isGenericComment(body)
  );
}

/** True when the selected code carries enough tokens to fingerprint meaningfully. */
export function hasSubstantiveCode(code: string): boolean {
  return (
    lexicalNormalizer.normalize(code).tokens.length >=
    MIN_SUBSTANTIVE_CODE_TOKENS
  );
}
