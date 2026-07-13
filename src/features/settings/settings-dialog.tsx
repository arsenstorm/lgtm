import {
  RiGithubLine,
  RiSettings4Line,
  RiSparkling2Line,
} from "@remixicon/react";
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

type SettingsSection = "general" | "github" | "memory";

const SECTIONS: { id: SettingsSection; label: string; icon: ReactNode }[] = [
  { id: "general", label: "General", icon: <RiSettings4Line aria-hidden /> },
  { id: "github", label: "GitHub", icon: <RiGithubLine aria-hidden /> },
  {
    id: "memory",
    label: "Reviewer memory",
    icon: <RiSparkling2Line aria-hidden />,
  },
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
        <nav className="flex w-44 shrink-0 flex-col gap-0.5 border-r py-2">
          <DialogTitle className="px-3 pt-1 pb-2 font-semibold text-sm">
            Settings
          </DialogTitle>
          <DialogDescription className="sr-only">
            Manage appearance, your GitHub connection, and reviewer memory.
          </DialogDescription>
          {SECTIONS.map((item) => (
            <button
              className={cn(
                "mx-1 flex items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring [&_svg]:size-4",
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
  control: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 py-3">
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="font-medium text-sm">{label}</span>
        {description ? (
          <span className="text-muted-foreground text-xs">{description}</span>
        ) : null}
      </div>
      <div className="shrink-0">{control}</div>
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
