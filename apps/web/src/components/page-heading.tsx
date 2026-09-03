import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

/** The title row every list page opens with: the name, a small figure beside
 *  it, and whatever trails at the end. It is as tall as a large button even
 *  when it holds none, so a page with an action starts its content on the
 *  same line as a page without one. */
export function PageHeading({
  children,
  className,
  meta,
  title,
}: {
  children?: ReactNode;
  className?: string;
  meta?: ReactNode;
  title: string;
}) {
  return (
    <div className={cn("flex min-h-9 items-center gap-3", className)}>
      <h1 className="font-medium text-xl tracking-tight">{title}</h1>
      {meta === undefined ? null : (
        <span className="text-muted-foreground text-sm tabular-nums">
          {meta}
        </span>
      )}
      {children ? (
        <div className="ms-auto flex items-center gap-2">{children}</div>
      ) : null}
    </div>
  );
}
