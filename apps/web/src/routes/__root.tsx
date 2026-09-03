import { createRootRoute, HeadContent, Scripts } from "@tanstack/react-router";

import { AppShell } from "@/components/app-shell";
import { Toaster } from "@/components/ui/sonner";
import { getTasks } from "@/lib/lgtm/server";
import type { Task } from "@/lib/lgtm/types";
import appCss from "../styles.css?url";

const THEME_INIT_SCRIPT = `(function(){try{var stored=window.localStorage.getItem('theme');var mode=(stored==='light'||stored==='dark')?stored:'auto';var prefersDark=window.matchMedia('(prefers-color-scheme: dark)').matches;var resolved=mode==='auto'?(prefersDark?'dark':'light'):mode;var root=document.documentElement;root.classList.remove('light','dark');root.classList.add(resolved);root.style.colorScheme=resolved;}catch(e){}})();`;

export const Route = createRootRoute({
  head: () => ({
    meta: [
      { charSet: "utf-8" },
      { name: "viewport", content: "width=device-width, initial-scale=1" },
      { title: "LGTM" },
    ],
    links: [{ rel: "stylesheet", href: appCss }],
  }),
  loader: async () => ({ tasks: await getTasks().catch(() => [] as Task[]) }),
  shellComponent: RootDocument,
});

function RootDocument({ children }: { children: React.ReactNode }) {
  const { tasks } = Route.useLoaderData();

  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_INIT_SCRIPT }} />
        <HeadContent />
      </head>
      <body className="font-sans antialiased">
        <AppShell tasks={tasks}>{children}</AppShell>
        <Toaster position="top-right" />
        <Scripts />
      </body>
    </html>
  );
}
