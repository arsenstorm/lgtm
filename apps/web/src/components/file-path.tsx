import { cn } from "@/lib/utils";

/** Dims the directory so the file name reads first. */
export function FilePath({
  path,
  className,
}: {
  path: string;
  className?: string;
}) {
  const cut = path.lastIndexOf("/") + 1;
  return (
    <span className={cn("font-mono [overflow-wrap:anywhere]", className)}>
      <span className="text-muted-foreground">{path.slice(0, cut)}</span>
      {path.slice(cut)}
    </span>
  );
}
