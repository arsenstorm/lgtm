import { createRootRoute, HeadContent, Scripts } from "@tanstack/react-router";
import { useEffect } from "react";

import { AppShell } from "@/components/app-shell";
import { NotFound } from "@/components/not-found";
import { Toaster } from "@/components/ui/sonner";
import { STRETCH_KEY, stretchDom } from "@/lib/lgtm/debug";
import { getChats, getProjects, getTasks } from "@/lib/lgtm/server";
import type { Chat, Project, Task } from "@/lib/lgtm/types";
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
    const [tasks, chats, projects] = await Promise.all([
      getTasks().catch(() => [] as Task[]),
      getChats().catch(() => [] as Chat[]),
      getProjects().catch(() => [] as Project[]),
    ]);
    return { chats, projects, tasks };
  },
  notFoundComponent: NotFound,
  shellComponent: RootDocument,
});

function RootDocument({ children }: { children: React.ReactNode }) {
  const { chats, projects, tasks } = Route.useLoaderData();
  useEffect(() => {
    if (import.meta.env.DEV && localStorage.getItem(STRETCH_KEY) === "1") {
      return stretchDom(document.body);
    }
  }, []);

  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_INIT_SCRIPT }} />
        <HeadContent />
      </head>
      <body className="font-sans antialiased">
        <AppShell chats={chats} projects={projects} tasks={tasks}>
          {children}
        </AppShell>
        <Toaster position="top-right" />
        <Scripts />
      </body>
    </html>
  );
}
