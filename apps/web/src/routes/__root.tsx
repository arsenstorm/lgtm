import { createRootRoute, HeadContent, Scripts } from "@tanstack/react-router";

import { AppShell } from "@/components/app-shell";
import { Toaster } from "@/components/ui/sonner";
import { getChats, getTasks } from "@/lib/lgtm/server";
import type { Chat, Task } from "@/lib/lgtm/types";
import appCss from "../styles.css?url";

const THEME_INIT_SCRIPT = `(function(){try{var stored=window.localStorage.getItem('theme');var mode=(stored==='light'||stored==='dark')?stored:'auto';var prefersDark=window.matchMedia('(prefers-color-scheme: dark)').matches;var resolved=mode==='auto'?(prefersDark?'dark':'light'):mode;var root=document.documentElement;root.classList.remove('light','dark');root.classList.add(resolved);root.dataset.theme=resolved;root.style.colorScheme=resolved;}catch(e){}})();`;

export const Route = createRootRoute({
  head: () => ({
    meta: [
      { charSet: "utf-8" },
      { name: "viewport", content: "width=device-width, initial-scale=1" },
      { title: "LGTM" },
    ],
    links: [{ rel: "stylesheet", href: appCss }],
  }),
  loader: async () => {
    const [tasks, chats] = await Promise.all([
      getTasks().catch(() => [] as Task[]),
      getChats().catch(() => [] as Chat[]),
    ]);
    return { chats, tasks };
  },
  shellComponent: RootDocument,
});

function RootDocument({ children }: { children: React.ReactNode }) {
  const { chats, tasks } = Route.useLoaderData();

  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_INIT_SCRIPT }} />
        <HeadContent />
      </head>
      <body className="font-sans antialiased">
        <AppShell chats={chats} tasks={tasks}>
          {children}
        </AppShell>
        <Toaster position="top-right" />
        <Scripts />
      </body>
    </html>
  );
}
