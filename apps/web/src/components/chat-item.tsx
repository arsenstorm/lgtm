import { WarningCircle } from "@phosphor-icons/react";
import { Link, useMatchRoute, useRouter } from "@tanstack/react-router";
import { useCallback } from "react";
import { toast } from "sonner";

import { DotsIcon, MsgsIcon } from "@/components/icons";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";
import { updateChat } from "@/lib/lgtm/server";
import type { Chat } from "@/lib/lgtm/types";

export function ChatItem({ chat }: { chat: Chat }) {
  const matchRoute = useMatchRoute();
  const router = useRouter();
  const failed = chat.turns.at(-1)?.failed ?? false;

  const update = useCallback(
    async (patch: { title?: string; archived?: boolean }) => {
      try {
        await updateChat({ data: { id: chat.id, ...patch } });
        await router.invalidate();
      } catch (error) {
        toast.error(error instanceof Error ? error.message : String(error));
      }
    },
    [chat.id, router]
  );

  const rename = useCallback(() => {
    // ponytail: window.prompt is the deliberate cheap path, the same one the
    // project prefix takes; a real dialog arrives with the settings surface.
    // biome-ignore lint/suspicious/noAlert: cheap path, see above
    const entered = window.prompt("Rename chat", chat.title);
    const title = entered?.trim();
    if (title && title !== chat.title) {
      update({ title });
    }
  }, [chat.title, update]);

  const archive = useCallback(async () => {
    await update({ archived: true });
    toast.success("Chat archived");
  }, [update]);

  return (
    <SidebarMenuItem>
      {/* The right padding keeps the title's truncation clear of the action
          that sits over this row. */}
      <SidebarMenuButton
        className="pr-8"
        isActive={!!matchRoute({ params: { id: chat.id }, to: "/chats/$id" })}
        render={<Link params={{ id: chat.id }} to="/chats/$id" />}
      >
        <MsgsIcon aria-hidden="true" />
        <span className="truncate">{chat.title}</span>
        {failed ? (
          // The menu takes this corner when the row is reached, so the mark
          // steps aside rather than showing through the dots. The menu is
          // portalled, so hover ends when it opens; `has-[aria-expanded]` on
          // the row keeps the mark away until it closes.
          <span
            aria-label="The last answer failed"
            className="ml-auto flex shrink-0 text-red-500 transition-opacity group-focus-within/menu-item:opacity-0 group-hover/menu-item:opacity-0 group-has-[[aria-expanded=true]]/menu-item:opacity-0"
            role="img"
          >
            <WarningCircle className="size-3.5" weight="fill" />
          </span>
        ) : null}
      </SidebarMenuButton>

      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <SidebarMenuAction
              aria-label={`Manage ${chat.title}`}
              className="text-muted-foreground"
              showOnHover
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
          <DropdownMenuItem className="gap-2 px-2 py-1.5" onClick={archive}>
            Archive
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </SidebarMenuItem>
  );
}
