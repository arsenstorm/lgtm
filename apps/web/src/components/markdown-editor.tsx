import Link from "@tiptap/extension-link";
import Placeholder from "@tiptap/extension-placeholder";
import TaskItem from "@tiptap/extension-task-item";
import TaskList from "@tiptap/extension-task-list";
import type { Editor } from "@tiptap/react";
import { EditorContent, useEditor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { useCallback, useEffect, useRef } from "react";
import { Markdown } from "tiptap-markdown";

import { cn } from "@/lib/utils";

export interface EditorHeading {
  /** Index among all headings, used by the TOC to scroll the matching DOM node. */
  index: number;
  /** 1–3 */
  level: number;
  text: string;
}

export interface MarkdownEditorProps {
  autoFocus?: boolean;
  /** Extra classes for the outer wrapper. */
  className?: string;
  /** Called with the editor's headings whenever they change; feeds the TOC. */
  onHeadings?: (headings: EditorHeading[]) => void;
  /** Debounced (800ms after last change) AND flushed immediately on blur. Only fires when the serialized markdown actually differs from the last emission. */
  onMarkdown: (markdown: string) => void;
  placeholder?: string;
  /** Initial markdown. The editor is uncontrolled after mount; remount (key) to reset. */
  value: string;
}

const DEBOUNCE_MS = 800;

/**
 * The TOC indexes headings by document order, and this reads them back the same
 * way: the nth heading in the DOM is the nth heading in the ProseMirror doc.
 * Keep both sides on document order or the indexes stop meaning anything.
 */
export function scrollToHeading(container: HTMLElement | null, index: number) {
  const node = container?.querySelectorAll("h1, h2, h3")[index];
  node?.scrollIntoView({ behavior: "smooth", block: "start" });
}

function readHeadings(editor: Editor): EditorHeading[] {
  const headings: EditorHeading[] = [];
  editor.state.doc.descendants((node) => {
    if (node.type.name === "heading") {
      headings.push({
        index: headings.length,
        level: node.attrs.level as number,
        text: node.textContent,
      });
    }
  });
  return headings;
}

const signature = (headings: EditorHeading[]) =>
  headings.map((h) => `${h.level}:${h.text}`).join("\n");

const CONTENT_CLASSES = [
  "[&_.ProseMirror]:outline-none [&_.ProseMirror]:min-h-40 [&_.ProseMirror]:text-sm [&_.ProseMirror]:leading-relaxed",
  // The document starts flush with its container: a heading's own top margin
  // is for the block above it, and there is none. Teleporting to a heading
  // leaves it a little clear of the header's divider.
  "[&_.ProseMirror>*:first-child]:mt-0 [&_.ProseMirror_:is(h1,h2,h3)]:scroll-mt-1",
  "[&_.ProseMirror>*+*]:mt-3",
  "[&_.ProseMirror_h1]:mt-6 [&_.ProseMirror_h1]:text-2xl [&_.ProseMirror_h1]:font-semibold [&_.ProseMirror_h1]:tracking-tight",
  "[&_.ProseMirror_h2]:mt-6 [&_.ProseMirror_h2]:text-lg [&_.ProseMirror_h2]:font-semibold [&_.ProseMirror_h2]:tracking-tight",
  "[&_.ProseMirror_h3]:mt-5 [&_.ProseMirror_h3]:text-base [&_.ProseMirror_h3]:font-semibold",
  "[&_.ProseMirror_ul]:list-disc [&_.ProseMirror_ul]:pl-6 [&_.ProseMirror_ol]:list-decimal [&_.ProseMirror_ol]:pl-6",
  "[&_.ProseMirror_li]:my-1 [&_.ProseMirror_li>p]:my-0",
  "[&_.ProseMirror_ul[data-type=taskList]]:list-none [&_.ProseMirror_ul[data-type=taskList]]:pl-0",
  "[&_.ProseMirror_li[data-type=taskItem]]:flex [&_.ProseMirror_li[data-type=taskItem]]:items-start [&_.ProseMirror_li[data-type=taskItem]]:gap-2",
  "[&_.ProseMirror_li[data-type=taskItem]>label]:mt-1 [&_.ProseMirror_li[data-type=taskItem]>label]:shrink-0 [&_.ProseMirror_li[data-type=taskItem]>div]:min-w-0",
  "[&_.ProseMirror_code]:rounded [&_.ProseMirror_code]:bg-muted [&_.ProseMirror_code]:px-1 [&_.ProseMirror_code]:py-0.5 [&_.ProseMirror_code]:font-mono [&_.ProseMirror_code]:text-sm",
  "[&_.ProseMirror_pre]:overflow-x-auto [&_.ProseMirror_pre]:rounded-lg [&_.ProseMirror_pre]:bg-muted [&_.ProseMirror_pre]:p-3 [&_.ProseMirror_pre]:font-mono [&_.ProseMirror_pre]:text-sm",
  "[&_.ProseMirror_pre_code]:bg-transparent [&_.ProseMirror_pre_code]:p-0",
  "[&_.ProseMirror_blockquote]:border-l-2 [&_.ProseMirror_blockquote]:border-border [&_.ProseMirror_blockquote]:pl-3 [&_.ProseMirror_blockquote]:text-muted-foreground",
  "[&_.ProseMirror_hr]:my-6 [&_.ProseMirror_hr]:border-border",
  "[&_.ProseMirror_a]:underline [&_.ProseMirror_a]:underline-offset-2",
  "[&_.ProseMirror_p.is-editor-empty:first-child]:before:pointer-events-none [&_.ProseMirror_p.is-editor-empty:first-child]:before:float-left [&_.ProseMirror_p.is-editor-empty:first-child]:before:h-0 [&_.ProseMirror_p.is-editor-empty:first-child]:before:text-muted-foreground [&_.ProseMirror_p.is-editor-empty:first-child]:before:content-[attr(data-placeholder)]",
].join(" ");

export function MarkdownEditor({
  value,
  onMarkdown,
  placeholder = "Write something…",
  autoFocus = false,
  className,
  onHeadings,
}: MarkdownEditorProps) {
  const onMarkdownRef = useRef(onMarkdown);
  onMarkdownRef.current = onMarkdown;
  const onHeadingsRef = useRef(onHeadings);
  onHeadingsRef.current = onHeadings;

  const timer = useRef<number | null>(null);
  const pending = useRef(value);
  const emitted = useRef(value);
  const headingSig = useRef<string | null>(null);

  const flush = useCallback(() => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
    if (pending.current === emitted.current) {
      return;
    }
    emitted.current = pending.current;
    onMarkdownRef.current(pending.current);
  }, []);

  const syncHeadings = useCallback((instance: Editor) => {
    if (!onHeadingsRef.current) {
      return;
    }
    const headings = readHeadings(instance);
    const sig = signature(headings);
    if (sig === headingSig.current) {
      return;
    }
    headingSig.current = sig;
    onHeadingsRef.current(headings);
  }, []);

  const editor = useEditor({
    autofocus: autoFocus,
    content: value,
    extensions: [
      StarterKit.configure({ heading: { levels: [1, 2, 3] } }),
      Link.configure({ openOnClick: false }),
      Placeholder.configure({ placeholder }),
      TaskList,
      TaskItem.configure({ nested: false }),
      Markdown.configure({ html: false, tightLists: true }),
    ],
    // Tiptap touches the DOM as it builds its view, so rendering it during SSR
    // throws on the worker; the client paints it with the loader's markdown on
    // the first pass instead.
    immediatelyRender: false,
    onBlur: flush,
    onCreate: ({ editor: created }) => syncHeadings(created),
    onUpdate: ({ editor: updated }) => {
      pending.current = updated.storage.markdown.getMarkdown();
      syncHeadings(updated);
      if (timer.current !== null) {
        window.clearTimeout(timer.current);
      }
      timer.current = window.setTimeout(flush, DEBOUNCE_MS);
    },
  });

  // An unmount with a pending debounce is a silently lost edit — a navigation
  // away within 800ms of the last keystroke must still save.
  useEffect(() => flush, [flush]);

  if (!editor) {
    return <div className={className} />;
  }

  return (
    <div className={cn("flex flex-col gap-2", className)}>
      <EditorContent className={CONTENT_CLASSES} editor={editor} />
    </div>
  );
}
