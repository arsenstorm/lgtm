import { useCallback } from "react";

import { DotsIcon } from "@/components/icons";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { SidebarMenuAction } from "@/components/ui/sidebar";
import { cn } from "@/lib/utils";

// The row's own hover group has to be named, because a project's task rows
// sit inside the project's `menu-item`: with the generic reveal, reaching one
// project would light up the dots on every task under it.
export const ROW_REVEAL =
  "group-focus-within/menu-item:opacity-100 group-hover/menu-item:opacity-100";
export const SUB_ROW_REVEAL =
  "top-1 group-focus-within/menu-sub-item:opacity-100 group-hover/menu-sub-item:opacity-100";

/** Rename and archive for one sidebar row, sitting over its right corner. */
export function RowMenu({
  className,
  onArchive,
  onRename,
  title,
}: {
  className: string;
  onArchive: () => void;
  onRename: (title: string) => void;
  title: string;
}) {
  const rename = useCallback(() => {
    // ponytail: window.prompt is the deliberate cheap path, the same one the
    // project prefix takes; a real dialog arrives with the settings surface.
    // biome-ignore lint/suspicious/noAlert: cheap path, see above
    const entered = window.prompt("Rename", title);
    const next = entered?.trim();
    if (next && next !== title) {
      onRename(next);
    }
  }, [onRename, title]);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <SidebarMenuAction
            aria-label={`Manage ${title}`}
            className={cn(
              "text-muted-foreground aria-expanded:opacity-100 md:opacity-0",
              className
            )}
          />
        }
      >
        <DotsIcon aria-hidden="true" className="size-4" />
      </DropdownMenuTrigger>

      <DropdownMenuContent
        align="start"
        className="w-40 rounded-lg"
        side="right"
        sideOffset={4}
      >
        <DropdownMenuItem className="gap-2 px-2 py-1.5" onClick={rename}>
          Rename…
        </DropdownMenuItem>
        <DropdownMenuItem className="gap-2 px-2 py-1.5" onClick={onArchive}>
          Archive
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
