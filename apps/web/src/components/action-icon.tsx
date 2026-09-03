import { type IconComponent, LoaderIcon } from "@/components/icons";

/** Swapping the leading icon for the spinner, rather than adding one, keeps the
 *  button the same width while it works. */
export function ActionIcon({
  icon: Glyph,
  busy,
}: {
  busy: boolean;
  icon: IconComponent;
}) {
  if (busy) {
    return (
      <LoaderIcon
        className="motion-safe:animate-spin"
        data-icon="inline-start"
      />
    );
  }
  return <Glyph data-icon="inline-start" />;
}
