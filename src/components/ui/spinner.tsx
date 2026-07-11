import { RiLoaderLine } from "@remixicon/react";
import { cn } from "cnfast";

function Spinner({ className, ...props }: React.ComponentProps<"svg">) {
  return (
    <RiLoaderLine
      aria-label="Loading"
      className={cn("size-4 animate-spin", className)}
      data-slot="spinner"
      role="status"
      {...props}
    />
  );
}

export { Spinner };
