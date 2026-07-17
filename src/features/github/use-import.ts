import { useCallback, useRef, useState } from "react";
import { toast } from "sonner";
import { insertImportedComments } from "@/lib/db/imported-comments";
import { type AppError, toAppError } from "@/lib/errors/app-error";
import { deriveExamplesFromImports } from "@/lib/github/import-memory";
import { importGithubReviewComments } from "@/lib/tauri/github";
import { runImport } from "./import-loop";

type ImportStatus = "idle" | "running" | "done" | "cancelled" | "error";

type ImportState = {
  status: ImportStatus;
  page: number;
  imported: number;
  derived: number;
  cappedWithMore: boolean;
  error: AppError | null;
};

const IDLE: ImportState = {
  status: "idle",
  page: 0,
  imported: 0,
  derived: 0,
  cappedWithMore: false,
  error: null,
};

/**
 * Runs the review-comment import for one repository, exposing progress and a
 * cooperative cancel. Dedup lives in the db layer, so a re-run after cancel or
 * error is safe.
 */
export function useImport(args: {
  repositoryId: string;
  owner: string;
  repository: string;
}) {
  const { repositoryId, owner, repository } = args;
  const [state, setState] = useState<ImportState>(IDLE);
  const cancelRef = useRef(false);

  const start = useCallback(async () => {
    cancelRef.current = false;
    setState({ ...IDLE, status: "running" });
    try {
      const outcome = await runImport({
        fetchPage: (page) =>
          importGithubReviewComments(owner, repository, page),
        insert: (comments) => insertImportedComments(repositoryId, comments),
        derive: (fresh) => deriveExamplesFromImports(repositoryId, fresh),
        isCancelled: () => cancelRef.current,
        onProgress: ({ page, imported, derived }) =>
          setState((prev) => ({ ...prev, page, imported, derived })),
      });
      setState({
        status: outcome.cancelled ? "cancelled" : "done",
        page: outcome.pagesFetched,
        imported: outcome.imported,
        derived: outcome.derived,
        cappedWithMore: outcome.cappedWithMore,
        error: null,
      });
    } catch (error) {
      const appError = toAppError(error);
      setState((prev) => ({ ...prev, status: "error", error: appError }));
      toast.error("Import stopped", { description: appError.message });
    }
  }, [repositoryId, owner, repository]);

  const cancel = useCallback(() => {
    cancelRef.current = true;
  }, []);

  const reset = useCallback(() => {
    cancelRef.current = false;
    setState(IDLE);
  }, []);

  return { ...state, start, cancel, reset };
}
