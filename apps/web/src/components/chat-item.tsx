import { Link, useMatchRoute, useRouter } from "@tanstack/react-router";
import { useCallback } from "react";
import { toast } from "sonner";

import { MsgsIcon, SquareWarningIcon } from "@/components/icons";
import { ROW_REVEAL, ROW_SLOT, RowMenu } from "@/components/row-menu";
import { SidebarMenuButton, SidebarMenuItem } from "@/components/ui/sidebar";
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
  const rename = useCallback((title: string) => update({ title }), [update]);
  const archive = useCallback(async () => {
    await update({ archived: true });
    toast.success("Chat archived");
  }, [update]);

  return (
    <SidebarMenuItem>
      <SidebarMenuButton
        isActive={!!matchRoute({ params: { id: chat.id }, to: "/chats/$id" })}
        render={<Link params={{ id: chat.id }} to="/chats/$id" />}
      >
        <MsgsIcon aria-hidden="true" />
        <span className="truncate">{chat.title}</span>
        <span className={ROW_SLOT}>
          {failed ? (
            // The menu takes this corner when the row is reached, so the mark
            // steps aside rather than showing through the dots. The menu is
            // portalled, so hover ends when it opens; `has-[aria-expanded]` on
            // the row keeps the mark away until it closes.
            <span
              aria-label="The last answer failed"
              className="flex text-red-500 transition-opacity group-focus-within/menu-item:opacity-0 group-hover/menu-item:opacity-0 group-has-[[aria-expanded=true]]/menu-item:opacity-0"
              role="img"
            >
              <SquareWarningIcon className="size-3.5" />
            </span>
          ) : null}
        </span>
      </SidebarMenuButton>
      <RowMenu
        className={ROW_REVEAL}
        onArchive={archive}
        onRename={rename}
        title={chat.title}
      />
    </SidebarMenuItem>
  );
}
