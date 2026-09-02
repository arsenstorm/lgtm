import { Link } from '@tanstack/react-router'
import { Check, ListChecks, Moon, Server, Sun } from 'lucide-react'

const NAV = [
  { to: '/', label: 'Tasks', icon: ListChecks, exact: true },
  { to: '/runners', label: 'Runners', icon: Server, exact: false },
] as const

// Nav links and the theme toggle share one row shape so the sidebar reads as a
// single column of controls rather than a list plus a stray button.
const ROW =
  'flex h-9 w-full items-center justify-center gap-2.5 rounded-md px-2.5 text-sm font-medium outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-ring/50 md:justify-start'

const ROW_IDLE = 'text-muted-foreground hover:bg-accent/60 hover:text-foreground'

/**
 * Mirrors the pre-paint script in `__root.tsx`: same `theme` key, same pair of
 * classes on <html>, same inline color-scheme. Drifting from it reintroduces
 * the flash that script exists to prevent.
 */
function toggleTheme() {
  const root = document.documentElement
  const next = root.classList.contains('dark') ? 'light' : 'dark'

  root.classList.remove('light', 'dark')
  root.classList.add(next)
  root.style.colorScheme = next

  try {
    window.localStorage.setItem('theme', next)
  } catch {
    // Storage can be unavailable (private mode, blocked cookies); the toggle
    // still works for this page, it just will not be remembered.
  }
}

export function AppShell({ children }: { children: React.ReactNode }) {
  return (
    <div className="isolate flex h-dvh overflow-hidden bg-background">
      <aside className="flex w-16 shrink-0 flex-col border-r border-border md:w-56">
        <div className="flex h-14 shrink-0 items-center justify-center gap-2 px-3 md:justify-start md:px-4">
          <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-foreground text-background">
            <Check className="size-3.5" strokeWidth={3} aria-hidden="true" />
          </span>
          <span className="hidden text-sm font-semibold tracking-tight md:block">
            LGTM
          </span>
        </div>

        <nav className="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2 py-2 md:px-3">
          {NAV.map(({ to, label, icon: Icon, exact }) => (
            <Link
              key={to}
              to={to}
              activeOptions={{ exact }}
              className={ROW}
              // The router appends these to `className` rather than replacing
              // it, so the two states must not both carry colour utilities.
              activeProps={{ className: 'bg-accent text-accent-foreground', 'aria-current': 'page' }}
              inactiveProps={{ className: ROW_IDLE }}
            >
              <Icon className="size-4 shrink-0" aria-hidden="true" />
              <span className="sr-only md:not-sr-only">{label}</span>
            </Link>
          ))}
        </nav>

        <div className="shrink-0 border-t border-border p-2 md:p-3">
          <button type="button" onClick={toggleTheme} className={`${ROW} ${ROW_IDLE}`}>
            {/* The theme is only known from the class the pre-paint script wrote,
                so both states ship in the markup and CSS picks one. Deriving it in
                React would mismatch on hydration and flash. */}
            <Moon className="size-4 shrink-0 dark:hidden" aria-hidden="true" />
            <Sun className="hidden size-4 shrink-0 dark:block" aria-hidden="true" />
            <span className="sr-only md:not-sr-only">
              <span className="dark:hidden">Dark mode</span>
              <span className="hidden dark:inline">Light mode</span>
            </span>
          </button>
        </div>
      </aside>

      <main className="min-w-0 flex-1 overflow-y-auto overscroll-contain">
        {children}
      </main>
    </div>
  )
}
