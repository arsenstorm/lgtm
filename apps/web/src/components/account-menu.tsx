import { useRouter } from "@tanstack/react-router";
import { useCallback, useEffect, useState } from "react";

import {
  DotsIcon,
  MoonStarsIcon,
  SignOutIcon,
  SunIcon,
} from "@/components/icons";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from "@/components/ui/sidebar";
import { DEBUG_COOKIE } from "@/lib/lgtm/debug";
import { setDebugMode } from "@/lib/lgtm/server";

/**
 * Mirrors the pre-paint script in `__root.tsx`: same `theme` key, same pair of
 * classes on <html>, same inline color-scheme. Drifting from it reintroduces
 * the flash that script exists to prevent.
 */
function toggleTheme() {
  const root = document.documentElement;
  const next = root.classList.contains("dark") ? "light" : "dark";

  // Hundreds of elements carry transition-colors; letting them all animate
  // makes the flip look staggered. The attribute suppresses transitions for
  // the switch and leaves once the new theme has painted.
  root.setAttribute("data-theme-switching", "");
  window.setTimeout(() => root.removeAttribute("data-theme-switching"), 80);
  root.classList.remove("light", "dark");
  root.classList.add(next);
  root.dataset.theme = next;
  root.style.colorScheme = next;

  try {
    window.localStorage.setItem("theme", next);
  } catch {
    // Storage can be unavailable (private mode, blocked cookies); the toggle
    // still works for this page, it just will not be remembered.
  }
}

// Placeholder identity: real auth (and a real signed-in user) lands later, so
// the name, email and initials are hard-coded and sign out is inert.
const USER = {
  email: "arsen@shkrumelyak.com",
  initials: "AS",
  name: "Arsen Shkrumelyak",
};

export function AccountMenu() {
  const { isMobile } = useSidebar();

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <SidebarMenuButton
                className="data-open:bg-sidebar-accent data-open:text-sidebar-accent-foreground"
                size="lg"
              />
            }
          >
            <Identity />
            <DotsIcon aria-hidden="true" className="ml-auto size-4" vertical />
          </DropdownMenuTrigger>

          <DropdownMenuContent
            align="end"
            className="min-w-56 rounded-lg"
            side={isMobile ? "bottom" : "right"}
            sideOffset={4}
          >
            <DropdownMenuGroup>
              {/* Base UI: a menu label must sit inside a menu group. */}
              <DropdownMenuLabel className="p-0 font-normal">
                <div className="flex items-center gap-2 px-1 py-1.5 text-left text-sm">
                  <Identity />
                </div>
              </DropdownMenuLabel>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            {/* base-nova's menu rows are tighter than the dashboard register this
                menu copies, so the rhythm is restated here. */}
            <DropdownMenuItem
              className="gap-2 px-2 py-1.5"
              // Toggling the theme is something you do to see the result; the
              // menu closing would hide the very thing being compared.
              closeOnClick={false}
              onClick={toggleTheme}
            >
              {/* The theme is only known from the class the pre-paint script
                  wrote, so both states ship in the markup and CSS picks one.
                  Deriving it in React would mismatch on hydration and flash. */}
              <MoonStarsIcon aria-hidden="true" className="dark:hidden" />
              <SunIcon aria-hidden="true" className="hidden dark:block" />
              <span className="dark:hidden">Dark mode</span>
              <span className="hidden dark:inline">Light mode</span>
            </DropdownMenuItem>
            {import.meta.env.DEV ? <StretchTextItem /> : null}
            <DropdownMenuSeparator />
            <DropdownMenuItem className="gap-2 px-2 py-1.5" disabled>
              <SignOutIcon aria-hidden="true" />
              Sign out
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  );
}

function Identity() {
  return (
    <>
      <Avatar className="rounded-lg after:rounded-lg">
        <AvatarFallback className="rounded-lg">{USER.initials}</AvatarFallback>
      </Avatar>
      <span className="grid min-w-0 flex-1 text-left text-sm leading-tight">
        <span className="truncate font-medium">{USER.name}</span>
        <span className="truncate text-muted-foreground text-xs">
          {USER.email}
        </span>
      </span>
    </>
  );
}

/** Dev only. Every string a person or an agent could have written comes back
 *  from the orchestrator much longer, so the layout's truncation and wrapping
 *  get exercised on real pages instead of on a fixture. */
function StretchTextItem() {
  const router = useRouter();
  const [on, setOn] = useState(false);
  useEffect(() => {
    setOn(document.cookie.includes(`${DEBUG_COOKIE}=1`));
  }, []);
  const toggle = useCallback(async () => {
    const next = !on;
    setOn(next);
    await setDebugMode({ data: next });
    await router.invalidate();
  }, [on, router]);

  return (
    <DropdownMenuCheckboxItem
      checked={on}
      className="gap-2 px-2 py-1.5"
      closeOnClick={false}
      onCheckedChange={toggle}
    >
      Stretch text
    </DropdownMenuCheckboxItem>
  );
}
