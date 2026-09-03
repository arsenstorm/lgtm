import type { TodoPriority } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

// The three bars are Arsen's icon set (src/icons/18-priority-*.svg), inlined
// so the level is a per-bar opacity instead of three separate assets: the
// shape stays put while the fill "climbs" with priority.
const DIMMED = 0.4;

const OPACITY: Record<TodoPriority, [number, number, number]> = {
  high: [1, 1, 1],
  medium: [1, 1, DIMMED],
  low: [1, DIMMED, DIMMED],
};

// Short, middle, tall — in the order the opacities above index them.
const BARS = [
  "M1.5 12.75C1.5 11.7835 2.2835 11 3.25 11H3.75C4.7165 11 5.5 11.7835 5.5 12.75V14.25C5.5 15.2165 4.7165 16 3.75 16H3.25C2.2835 16 1.5 15.2165 1.5 14.25V12.75Z",
  "M7 8.75C7 7.7835 7.7835 7 8.75 7H9.25C10.2165 7 11 7.7835 11 8.75V14.25C11 15.2165 10.2165 16 9.25 16H8.75C7.7835 16 7 15.2165 7 14.25V8.75Z",
  "M12.5 3.75C12.5 2.7835 13.2835 2 14.25 2H14.75C15.7165 2 16.5 2.7835 16.5 3.75V14.25C16.5 15.2165 15.7165 16 14.75 16H14.25C13.2835 16 12.5 15.2165 12.5 14.25V3.75Z",
];

export function PriorityIcon({
  priority,
  className,
  label,
}: {
  priority: TodoPriority;
  className?: string;
  /** When set the icon is the row's only statement of priority, so it speaks. */
  label?: string;
}) {
  const opacity = OPACITY[priority];

  return (
    <svg
      aria-hidden={label ? undefined : "true"}
      aria-label={label}
      className={cn("shrink-0", className)}
      role={label ? "img" : undefined}
      fill="currentColor"
      viewBox="0 0 18 18"
      xmlns="http://www.w3.org/2000/svg"
    >
      {BARS.map((d, index) => (
        <path
          clipRule="evenodd"
          d={d}
          fillOpacity={opacity[index]}
          fillRule="evenodd"
          key={d}
        />
      ))}
    </svg>
  );
}
