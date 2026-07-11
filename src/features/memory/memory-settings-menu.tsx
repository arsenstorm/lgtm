import { RiSparkling2Line } from "@remixicon/react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useMemoryCollection } from "./use-memory-collection";

/**
 * Header control for reviewer-memory collection. Self-contained: reads and
 * persists the "remember my comments" setting so nothing has to be drilled
 * through the shell.
 */
export function MemorySettingsMenu() {
  const { enabled, toggle } = useMemoryCollection();

  return (
    <DropdownMenu>
      <Tooltip>
        <TooltipTrigger
          render={
            <DropdownMenuTrigger
              render={
                <Button
                  aria-label="Memory settings"
                  size="icon-sm"
                  variant="ghost"
                >
                  <RiSparkling2Line aria-hidden />
                </Button>
              }
            />
          }
        />
        <TooltipContent>Memory settings</TooltipContent>
      </Tooltip>
      <DropdownMenuContent align="end" className="w-64">
        <DropdownMenuLabel>Reviewer memory</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuCheckboxItem
          checked={enabled}
          closeOnClick={false}
          onCheckedChange={(next) => {
            toggle(next).catch(() => {
              // Setting write failed; the optimistic UI is corrected on reload.
            });
          }}
        >
          Remember my comments
        </DropdownMenuCheckboxItem>
        <p className="px-2 py-1.5 text-muted-foreground text-xs">
          When on, your review comments seed suggestions on similar code later.
        </p>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
