import { Circle, CircleHalf } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import {
  ChevronIcon,
  CircleCheckIcon,
  type IconComponent,
} from "@/components/icons";
import { PriorityIcon } from "@/components/priority-icon";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { TodoPriority, TodoStatus } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

export const MARK: Record<
  TodoStatus,
  { icon: IconComponent; label: string; className: string }
> = {
  done: {
    className: "text-emerald-700 dark:text-emerald-400",
    icon: CircleCheckIcon,
    label: "Done",
  },
  in_progress: {
    className: "text-amber-600 dark:text-amber-400",
    icon: CircleHalf,
    label: "In progress",
  },
  open: { className: "text-muted-foreground", icon: Circle, label: "Open" },
};

const STATUS_OPTIONS: TodoStatus[] = ["open", "in_progress", "done"];
const PRIORITY_OPTIONS: TodoPriority[] = ["low", "medium", "high"];

export const PRIORITY: Record<
  TodoPriority,
  { className: string; label: string }
> = {
  high: {
    className: "border-red-600/30 text-red-700 dark:text-red-400",
    label: "High priority",
  },
  low: {
    className: "border-border text-muted-foreground",
    label: "Low priority",
  },
  medium: {
    className: "border-amber-600/30 text-amber-700 dark:text-amber-400",
    label: "Medium priority",
  },
};

export const CHIP =
  "inline-flex min-w-0 max-w-full items-center gap-1.5 overflow-hidden whitespace-nowrap rounded-full border px-2.5 py-0.5 text-xs [&_svg]:size-3.5 [&_svg]:shrink-0";

export function StatusChip({
  value,
  disabled,
  onPick,
}: {
  disabled: boolean;
  onPick: (value: TodoStatus) => void;
  value: TodoStatus;
}) {
  const { icon: Mark, label, className } = MARK[value];

  return (
    <Picker
      disabled={disabled}
      format={(status) => MARK[status].label}
      onPick={onPick}
      options={STATUS_OPTIONS}
      triggerClassName={cn("border-border", className)}
      value={value}
    >
      <Mark />
      <span className="truncate">{label}</span>
    </Picker>
  );
}

export function PriorityChip({
  value,
  disabled,
  onPick,
}: {
  disabled: boolean;
  onPick: (value: TodoPriority) => void;
  value: TodoPriority;
}) {
  const { className, label } = PRIORITY[value];

  return (
    <Picker
      disabled={disabled}
      format={(priority) => PRIORITY[priority].label}
      onPick={onPick}
      options={PRIORITY_OPTIONS}
      triggerClassName={className}
      value={value}
    >
      <PriorityIcon className="size-3.5" priority={value} />
      <span className="truncate">{label}</span>
    </Picker>
  );
}

function Picker<T extends string>({
  value,
  options,
  format,
  disabled,
  onPick,
  triggerClassName,
  children,
}: {
  children: ReactNode;
  disabled: boolean;
  format: (value: T) => string;
  onPick: (value: T) => void;
  options: T[];
  triggerClassName: string;
  value: T;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label={format(value)}
        className={cn(
          CHIP,
          "transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50",
          triggerClassName
        )}
        disabled={disabled}
      >
        {children}
        <ChevronIcon className="text-muted-foreground" direction="down" />
      </DropdownMenuTrigger>
      <DropdownMenuContent className="min-w-40">
        <DropdownMenuRadioGroup
          onValueChange={(next) => {
            if (next !== value) {
              onPick(next as T);
            }
          }}
          value={value}
        >
          {options.map((option) => (
            <DropdownMenuRadioItem key={option} value={option}>
              <span>{format(option)}</span>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
