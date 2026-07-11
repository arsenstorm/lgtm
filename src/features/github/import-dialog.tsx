import { RiDownloadLine } from "@remixicon/react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Spinner } from "@/components/ui/spinner";
import { useImport } from "./use-import";

type ImportDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  repositoryId: string;
  owner: string;
  repository: string;
};

/**
 * Explicit, cancellable import of the reviewer's past GitHub review comments on
 * this repository into reviewer memory. Deduped in the db layer, so re-running
 * after a cancel or error is safe.
 */
export function ImportDialog({
  open,
  onOpenChange,
  repositoryId,
  owner,
  repository,
}: ImportDialogProps) {
  const importer = useImport({ repositoryId, owner, repository });
  const running = importer.status === "running";

  const close = (next: boolean) => {
    if (running) {
      return;
    }
    if (!next) {
      importer.reset();
    }
    onOpenChange(next);
  };

  return (
    <Dialog onOpenChange={close} open={open}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <RiDownloadLine aria-hidden className="size-4" />
            Import my review comments
          </DialogTitle>
          <DialogDescription>
            Pull your past review comments on{" "}
            <span className="font-medium">
              {owner}/{repository}
            </span>
            . They are stored locally and filtered into reviewer memory to seed
            suggestions on similar code.
          </DialogDescription>
        </DialogHeader>

        <Body importer={importer} />

        <DialogFooter>
          {running ? (
            <Button onClick={importer.cancel} variant="outline">
              Cancel
            </Button>
          ) : (
            <>
              <Button onClick={() => close(false)} variant="outline">
                Close
              </Button>
              <Button onClick={importer.start}>
                {importer.status === "idle" ? "Start import" : "Run again"}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Body({ importer }: { importer: ReturnType<typeof useImport> }) {
  if (importer.status === "running") {
    return (
      <p className="flex items-center gap-2 text-sm">
        <Spinner />
        Page {importer.page} — {importer.imported} comment
        {importer.imported === 1 ? "" : "s"} imported so far
      </p>
    );
  }

  if (importer.status === "done" || importer.status === "cancelled") {
    return (
      <div className="flex flex-col gap-1 text-sm">
        <p>
          {importer.imported} imported, {importer.derived} became memory example
          {importer.derived === 1 ? "" : "s"}.
        </p>
        {importer.status === "cancelled" ? (
          <p className="text-muted-foreground text-xs">Import cancelled.</p>
        ) : null}
        {importer.cappedWithMore ? (
          <p className="text-muted-foreground text-xs">
            Stopped at the 1000 most recent comments.
          </p>
        ) : null}
      </div>
    );
  }

  if (importer.status === "error") {
    return (
      <p className="text-sm">
        Import stopped early. {importer.imported} imported so far — running
        again is safe and resumes from what's missing.
      </p>
    );
  }

  return (
    <p className="text-muted-foreground text-sm">
      This scans up to the 1000 most recent comments.
    </p>
  );
}
