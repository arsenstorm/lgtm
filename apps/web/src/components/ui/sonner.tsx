import { useTheme } from "next-themes";
import { Toaster as Sonner, type ToasterProps } from "sonner";
import {
  CircleCheckIcon,
  CircleInfoIcon,
  CircleXIcon,
  LoaderIcon,
  WarningIcon,
} from "@/components/icons";

const Toaster = ({ ...props }: ToasterProps) => {
  const { theme = "system" } = useTheme();

  return (
    <Sonner
      className="toaster group"
      icons={{
        error: <CircleXIcon className="size-4" />,
        info: <CircleInfoIcon className="size-4" />,
        loading: <LoaderIcon className="size-4 animate-spin" />,
        success: <CircleCheckIcon className="size-4" />,
        warning: <WarningIcon className="size-4" />,
      }}
      style={
        {
          "--border-radius": "var(--radius)",
          "--normal-bg": "var(--popover)",
          "--normal-border": "var(--border)",
          "--normal-text": "var(--popover-foreground)",
        } as React.CSSProperties
      }
      theme={theme as ToasterProps["theme"]}
      toastOptions={{
        classNames: {
          toast: "cn-toast",
        },
      }}
      {...props}
    />
  );
};

export { Toaster };
