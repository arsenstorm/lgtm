import {
  RiGithubLine,
  RiKeyboardLine,
  RiQuestionLine,
  RiSettings4Line,
  RiSparkling2Line,
} from "@remixicon/react";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { cn } from "cnfast";
import { useTheme } from "next-themes";
import { type ReactNode, useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { Kbd, KbdGroup } from "@/components/ui/kbd";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { useGithubAuth } from "@/features/github/use-github-auth";
import { useMemoryCollection } from "@/features/memory/use-memory-collection";

type SettingsSection = "general" | "github" | "memory" | "keybinds" | "help";

const SECTIONS: { id: SettingsSection; label: string; icon: ReactNode }[] = [
  { id: "general", label: "General", icon: <RiSettings4Line aria-hidden /> },
  { id: "github", label: "GitHub", icon: <RiGithubLine aria-hidden /> },
  {
    id: "memory",
    label: "Memory",
    icon: <RiSparkling2Line aria-hidden />,
  },
  { id: "keybinds", label: "Keybinds", icon: <RiKeyboardLine aria-hidden /> },
  { id: "help", label: "Help", icon: <RiQuestionLine aria-hidden /> },
];

// Mirrors the review shortcuts registered in src/app/app-shell.tsx.
const KEYBINDS: { keys: string[]; label: string }[] = [
  { keys: ["J"], label: "Next file" },
  { keys: ["K"], label: "Previous file" },
  { keys: ["N"], label: "Next comment" },
  { keys: ["P"], label: "Previous comment" },
  { keys: ["C"], label: "Comment on selected lines" },
  { keys: ["V"], label: "Toggle file viewed" },
  { keys: ["R"], label: "Refresh diff" },
  { keys: ["S"], label: "Open review summary" },
  { keys: ["⌘", "K"], label: "Command palette" },
];

const THEME_OPTIONS = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
] as const;

const THEME_LABELS: Record<string, string> = {
  system: "System",
  light: "Light",
  dark: "Dark",
};

type SettingsDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Opens the existing GitHub connection dialog (TokenDialog) on top. */
  onManageGithub: () => void;
};

export function SettingsDialog({
  open,
  onOpenChange,
  onManageGithub,
}: SettingsDialogProps) {
  const [section, setSection] = useState<SettingsSection>("general");

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent className="flex gap-0 overflow-hidden p-0 sm:max-w-2xl">
        <nav className="flex w-44 shrink-0 flex-col gap-0.5 border-r px-6 py-4">
          <DialogTitle className="pb-2 font-semibold text-sm">
            Settings
          </DialogTitle>
          <DialogDescription className="sr-only">
            Manage appearance, your GitHub connection, and reviewer memory.
          </DialogDescription>
          {SECTIONS.map((item) => (
            <button
              className={cn(
                "-mx-3.5 flex items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring [&_svg]:size-4",
                section === item.id
                  ? "bg-muted text-foreground"
                  : "text-muted-foreground hover:bg-muted/60 hover:text-foreground"
              )}
              key={item.id}
              onClick={() => setSection(item.id)}
              type="button"
            >
              {item.icon}
              {item.label}
            </button>
          ))}
        </nav>

        <div className="h-[420px] flex-1 overflow-y-auto px-6 py-4">
          <h2 className="pr-8 pb-2 font-semibold text-base">
            {SECTIONS.find((item) => item.id === section)?.label}
          </h2>
          {section === "general" ? <GeneralSection /> : null}
          {section === "github" ? (
            <GithubSection onManageGithub={onManageGithub} open={open} />
          ) : null}
          {section === "memory" ? <MemorySection /> : null}
          {section === "keybinds" ? <KeybindsSection /> : null}
          {section === "help" ? <HelpSection /> : null}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function SettingsRow({
  label,
  description,
  control,
}: {
  label: string;
  description?: string;
  control?: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 py-3">
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="font-medium text-sm">{label}</span>
        {description ? (
          <span className="text-muted-foreground text-xs">{description}</span>
        ) : null}
      </div>
      {control ? <div className="shrink-0">{control}</div> : null}
    </div>
  );
}

function GeneralSection() {
  const { theme, setTheme } = useTheme();

  return (
    <div className="divide-y">
      <SettingsRow
        control={
          <Select
            onValueChange={(value) => setTheme(value ?? "system")}
            value={theme ?? "system"}
          >
            <SelectTrigger className="w-36" size="sm">
              <SelectValue>
                {(value: string | null) => THEME_LABELS[value ?? "system"]}
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              {THEME_OPTIONS.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        }
        label="Appearance"
      />
    </div>
  );
}

function GithubSection({
  open,
  onManageGithub,
}: {
  open: boolean;
  onManageGithub: () => void;
}) {
  const { status, login, refresh } = useGithubAuth();

  useEffect(() => {
    if (open) {
      refresh();
    }
  }, [open, refresh]);

  const connected = status === "connected" && login;

  return (
    <div className="divide-y">
      <SettingsRow
        control={
          <Button onClick={onManageGithub} size="sm" variant="outline">
            Manage…
          </Button>
        }
        description={
          connected
            ? `Connected as ${login}`
            : "Connect, replace, or disconnect your GitHub account."
        }
        label="GitHub account"
      />
    </div>
  );
}

function MemorySection() {
  const { enabled, toggle } = useMemoryCollection();

  return (
    <div className="divide-y">
      <SettingsRow
        control={
          <Switch
            checked={enabled}
            onCheckedChange={(next) => {
              toggle(next).catch(() => {
                // Setting write failed; optimistic UI is corrected on reload.
              });
            }}
          />
        }
        description="Your review comments seed suggestions on similar code later."
        label="Remember my comments"
      />
    </div>
  );
}

function KeybindsSection() {
  return (
    <div>
      <div className="divide-y">
        {KEYBINDS.map((bind) => (
          <div
            className="flex items-center justify-between gap-4 py-2.5"
            key={bind.label}
          >
            <span className="text-sm">{bind.label}</span>
            <KbdGroup>
              {bind.keys.map((chunk) => (
                <Kbd key={chunk}>{chunk}</Kbd>
              ))}
            </KbdGroup>
          </div>
        ))}
      </div>
      <p className="pt-3 text-muted-foreground text-xs">
        Shortcuts are active in the review workspace and never fire while you're
        typing.
      </p>
    </div>
  );
}

function HelpSection() {
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => {
        // Version lookup failed; fall back to the bare product name.
      });
  }, []);

  return (
    <div>
      <div className="divide-y">
        <SettingsRow
          description="Drag across the code or the line-number gutter, then press C."
          label="Select lines to comment"
        />
        <SettingsRow
          description="Comments are saved on your machine and only published when you submit a review."
          label="Reviews stay local"
        />
        <SettingsRow
          description="The diff refreshes automatically when the window regains focus, or press R."
          label="Keeping the diff fresh"
        />
        <SettingsRow
          control={
            <Button
              onClick={() => {
                openUrl("https://github.com/arsenstorm/lgtm").catch(() => {
                  // Opening the browser failed; nothing else to do here.
                });
              }}
              size="sm"
              variant="outline"
            >
              Open GitHub
            </Button>
          }
          description="LGTM is developed on GitHub."
          label="Source and issues"
        />
      </div>
      <p className="pt-3 text-muted-foreground text-xs">
        LGTM{version ? ` ${version}` : ""}
      </p>
    </div>
  );
}
