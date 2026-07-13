import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from "@/components/ui/command";
import { Kbd, KbdGroup } from "@/components/ui/kbd";
import type { ReviewAction } from "./use-review-shortcuts";

type CommandPaletteProps = {
  actions: ReviewAction[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

/** cmdk palette listing every review action with its keyboard shortcut. */
export function CommandPalette({
  actions,
  open,
  onOpenChange,
}: CommandPaletteProps) {
  return (
    <CommandDialog onOpenChange={onOpenChange} open={open}>
      <Command>
        <CommandInput placeholder="Run a review action…" />
        <CommandList>
          <CommandEmpty>No matching action.</CommandEmpty>
          <CommandGroup heading="Review">
            {/* Disabled actions are hidden, not greyed out. */}
            {actions
              .filter((action) => !action.disabled)
              .map((action) => (
                <CommandItem
                  key={action.id}
                  onSelect={() => {
                    onOpenChange(false);
                    action.run();
                  }}
                  value={action.label}
                >
                  {action.label}
                  {action.hint.length > 0 ? (
                    <CommandShortcut>
                      <KbdGroup>
                        {action.hint.map((chunk) => (
                          <Kbd key={chunk}>{chunk}</Kbd>
                        ))}
                      </KbdGroup>
                    </CommandShortcut>
                  ) : null}
                </CommandItem>
              ))}
          </CommandGroup>
        </CommandList>
      </Command>
    </CommandDialog>
  );
}
