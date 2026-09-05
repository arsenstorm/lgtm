import type { ReactNode } from "react";

import { ChevronIcon } from "@/components/icons";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";

/** A collapsible band of rows under a filled header: the label, then how
 *  many rows it holds. Rows indent with `pl-7` to sit under the label. */
export function ListGroup({
  children,
  count,
  label,
  open = true,
}: {
  children: ReactNode;
  count: number;
  label: string;
  /** Whether the group starts unfolded. */
  open?: boolean;
}) {
  return (
    <Collapsible defaultOpen={open}>
      <CollapsibleTrigger className="group/header flex w-full items-center gap-2 rounded-md bg-foreground/5 px-2 py-1.5 text-sm outline-none hover:bg-foreground/10 focus-visible:ring-2 focus-visible:ring-ring/50">
        <ChevronIcon
          aria-hidden="true"
          className="size-3 text-muted-foreground transition-transform duration-200 group-data-[panel-open]/header:rotate-90"
        />
        <span className="truncate font-medium">{label}</span>
        <span className="truncate text-muted-foreground tabular-nums">
          {count}
        </span>
      </CollapsibleTrigger>

      <CollapsibleContent>{children}</CollapsibleContent>
    </Collapsible>
  );
}
