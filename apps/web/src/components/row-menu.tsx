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

// The row's right corner is a slot the status mark centres in and the menu
// sits exactly over, so the title truncates before either and the dots land
// where the mark was. It only takes its width while something is in it: a
// mark, or the menu on hover, focus, open, and on small screens where the
// menu is always shown.
const SLOT =
  "ml-auto flex w-0 shrink-0 justify-center overflow-hidden has-[[role=img]]:w-5 max-md:w-5";
export const ROW_SLOT = `${SLOT} group-focus-within/menu-item:w-5 group-hover/menu-item:w-5 group-has-[[aria-expanded=true]]/menu-item:w-5`;
export const SUB_ROW_SLOT = `${SLOT} group-focus-within/menu-sub-item:w-5 group-hover/menu-sub-item:w-5 group-has-[[aria-expanded=true]]/menu-sub-item:w-5`;

// The row's own hover group has to be named, because a project's task rows
// sit inside the project's `menu-item`: with the generic reveal, reaching one
// project would light up the dots on every task under it.
export const ROW_REVEAL =
  "group-focus-within/menu-item:opacity-100 group-hover/menu-item:opacity-100";
// The sub button sits a pixel left of its item (`-translate-x-px`), so the
// menu over it shifts the same pixel to land on the mark's centre.
export const SUB_ROW_REVEAL =
  "-translate-x-px top-1 group-focus-within/menu-sub-item:opacity-100 group-hover/menu-sub-item:opacity-100";

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
              "right-2 text-muted-foreground aria-expanded:opacity-100 md:opacity-0",
              className
            )}
          />
        }
      >
        {/* The action sizes every child svg to 16px with a parent selector that
            outranks the glyph's own class; the marks it overlays are 14px. */}
        <DotsIcon aria-hidden="true" className="size-3.5!" />
      </DropdownMenuTrigger>

      <DropdownMenuContent
        align="start"
        className="w-40 rounded-lg"
        side="right"
        sideOffset={4}
      >
        <DropdownMenuItem className="gap-2 px-2 py-1.5" onClick={rename}>
          <span>Rename…</span>
        </DropdownMenuItem>
        <DropdownMenuItem className="gap-2 px-2 py-1.5" onClick={onArchive}>
          <span>Archive</span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
