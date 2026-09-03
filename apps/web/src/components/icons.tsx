import type { SVGProps } from "react";

// Arsen's icon set (src/icons/18-*.svg), inlined as components so they take
// currentColor and Tailwind sizing like the Phosphor icons they replace. The
// set grows as he adds files; keep one component per glyph, same order as the
// directory.

type IconProps = SVGProps<SVGSVGElement>;

function Base(props: IconProps) {
  return (
    <svg
      aria-hidden={props["aria-label"] ? undefined : "true"}
      fill="none"
      viewBox="0 0 18 18"
      xmlns="http://www.w3.org/2000/svg"
      {...props}
    />
  );
}

const STROKE = {
  stroke: "currentColor",
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  strokeWidth: 1.5,
};

/** Closed and open share the closed body so toggling never shifts the row. */
export function FolderIcon({ open, ...props }: { open?: boolean } & IconProps) {
  if (open) {
    return (
      <Base {...props}>
        <path
          d="M16.148 13.27L16.991 10.14C17.248 9.187 16.53 8.25 15.543 8.25H6.15001C5.47201 8.25 4.87801 8.705 4.70201 9.36L3.76001 12.86C3.50301 13.813 4.22101 14.75 5.20801 14.75H14.217C15.121 14.75 15.913 14.143 16.148 13.27Z"
          fill="currentColor"
          fillOpacity="0.3"
        />
        <path
          {...STROKE}
          d="M5 14.75H4.25C3.145 14.75 2.25 13.855 2.25 12.75V4.75C2.25 3.645 3.145 2.75 4.25 2.75H6.075C6.662 2.75 7.219 3.008 7.599 3.455L9.123 5.25H13.749C14.854 5.25 15.749 6.145 15.749 7.25V8.25"
        />
        <path
          {...STROKE}
          d="M16.148 13.27L16.991 10.14C17.248 9.187 16.53 8.25 15.543 8.25H6.15001C5.47201 8.25 4.87801 8.705 4.70201 9.36L3.76001 12.86C3.50301 13.813 4.22101 14.75 5.20801 14.75H14.217C15.121 14.75 15.913 14.143 16.148 13.27Z"
        />
      </Base>
    );
  }
  return (
    <Base {...props}>
      <path
        d="M13.75 5.25C14.855 5.25 15.75 6.145 15.75 7.25V12.75C15.75 13.855 14.855 14.75 13.75 14.75H4.25C3.145 14.75 2.25 13.855 2.25 12.75V4.75C2.25 3.645 3.145 2.75 4.25 2.75H6.075C6.662 2.75 7.219 3.008 7.599 3.455L9.123 5.25H13.749H13.75Z"
        fill="currentColor"
        fillOpacity="0.3"
      />
      <path
        {...STROKE}
        d="M13.75 5.25C14.855 5.25 15.75 6.145 15.75 7.25V12.75C15.75 13.855 14.855 14.75 13.75 14.75H4.25C3.145 14.75 2.25 13.855 2.25 12.75V4.75C2.25 3.645 3.145 2.75 4.25 2.75H6.075C6.662 2.75 7.219 3.008 7.599 3.455L9.123 5.25H13.749H13.75Z"
      />
    </Base>
  );
}

export function TasksIcon(props: IconProps) {
  return (
    <Base {...props}>
      <path
        d="M13.75 5.25H7.25C6.145 5.25 5.25 6.145 5.25 7.25V13.75C5.25 14.855 6.145 15.75 7.25 15.75H13.75C14.855 15.75 15.75 14.855 15.75 13.75V7.25C15.75 6.145 14.855 5.25 13.75 5.25Z"
        fill="currentColor"
        fillOpacity="0.3"
      />
      <path {...STROKE} d="M7.99695 11.25L9.60596 12.75L13.003 8.25" />
      <path
        {...STROKE}
        d="M13.75 5.25H7.25C6.145 5.25 5.25 6.145 5.25 7.25V13.75C5.25 14.855 6.145 15.75 7.25 15.75H13.75C14.855 15.75 15.75 14.855 15.75 13.75V7.25C15.75 6.145 14.855 5.25 13.75 5.25Z"
      />
      <path
        {...STROKE}
        d="M12.4012 2.74996C12.0022 2.06146 11.2151 1.64841 10.38 1.77291L3.45602 2.80196C2.36402 2.96386 1.61003 3.98093 1.77203 5.07393L2.75002 11.6547"
      />
    </Base>
  );
}

/** `show` points the inner arrow outward; the frame is identical. */
export function SidebarToggleIcon({
  show,
  ...props
}: { show?: boolean } & IconProps) {
  return (
    <Base {...props}>
      <path
        d={
          show
            ? "M10.78,5.97c-.293-.293-.768-.293-1.061,0s-.293,.768,0,1.061l1.97,1.97-1.97,1.97c-.293,.293-.293,.768,0,1.061,.146,.146,.338,.22,.53,.22s.384-.073,.53-.22l2.5-2.5c.293-.293,.293-.768,0-1.061l-2.5-2.5Z"
            : "M12.78,5.97c-.293-.293-.768-.293-1.061,0l-2.5,2.5c-.293,.293-.293,.768,0,1.061l2.5,2.5c.146,.146,.338,.22,.53,.22s.384-.073,.53-.22c.293-.293,.293-.768,0-1.061l-1.97-1.97,1.97-1.97c.293-.293,.293-.768,0-1.061Z"
        }
        fill="currentColor"
      />
      <path
        d="M14.25,2H3.75c-1.517,0-2.75,1.233-2.75,2.75V13.25c0,1.517,1.233,2.75,2.75,2.75H14.25c1.517,0,2.75-1.233,2.75-2.75V4.75c0-1.517-1.233-2.75-2.75-2.75Zm1.25,11.25c0,.689-.561,1.25-1.25,1.25H7V3.5h7.25c.689,0,1.25,.561,1.25,1.25V13.25Z"
        fill="currentColor"
      />
    </Base>
  );
}
