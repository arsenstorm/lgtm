import type { Icon } from "@phosphor-icons/react";
import { CircleNotch } from "@phosphor-icons/react";

/** Swapping the leading icon for the spinner, rather than adding one, keeps the
 *  button the same width while it works. */
export function ActionIcon({
  icon: Glyph,
  busy,
}: {
  busy: boolean;
  icon: Icon;
}) {
  if (busy) {
    return (
      <CircleNotch
        className="motion-safe:animate-spin"
        data-icon="inline-start"
      />
    );
  }
  return <Glyph data-icon="inline-start" />;
}
