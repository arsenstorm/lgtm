export type AppErrorCode =
  | "repository-not-found"
  | "not-a-git-repository"
  | "git-unavailable"
  | "git-command-failed"
  | "git-timeout"
  | "diff-too-large"
  | "invalid-argument"
  | "database-failure"
  | "internal"
  | "authentication-failed"
  | "github-rate-limited"
  | "github-permission-denied"
  | "pull-request-not-found"
  | "repository-not-accessible"
  | "pull-request-revision-changed"
  | "network-failed";

export type AppError = {
  code: AppErrorCode;
  message: string;
  details?: string;
};

export function isAppError(value: unknown): value is AppError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.code === "string" && typeof candidate.message === "string"
  );
}

export function toAppError(value: unknown): AppError {
  if (isAppError(value)) {
    return value;
  }
  if (value instanceof Error) {
    return { code: "internal", message: value.message };
  }
  return { code: "internal", message: String(value) };
}
