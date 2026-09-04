import type { ReactNode } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import { toast } from "sonner";

import { TextResponse } from "@/components/aicss/TextResponse";
import { ThinkingState } from "@/components/aicss/ThinkingState";
import type { Referenced } from "@/components/answer-references";
import { AnswerReferences } from "@/components/answer-references";
import { ChevronIcon } from "@/components/icons";
import { shortSpan } from "@/components/task-list";
import { Bubble, BubbleContent } from "@/components/ui/bubble";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import {
  Message,
  MessageContent,
  MessageFooter,
} from "@/components/ui/message";
import { createChat } from "@/lib/lgtm/server";
import type { Chat, ChatStep, ChatTurn } from "@/lib/lgtm/types";

export type ComposerMode = "chat" | "task";

/** The conversation as the models read it when writing a task brief. */
export function transcript(turns: ChatTurn[]): string {
  return turns
    .map(
      (item) => `${item.role === "person" ? "Person" : "Agent"}: ${item.text}`
    )
    .join("\n\n");
}

/** Chat is the default: reading the workspace costs nothing. Task mode is the
 * explicit step that can queue work, so it is the one a person has to pick.
 * `pending` is shared with the composer's own actions so one thing runs at a
 * time. */
export function useWorkspaceChat({
  initialMode,
  pending,
  setPending,
}: {
  initialMode: ComposerMode;
  pending: string | null;
  setPending: (next: "ask" | null) => void;
}) {
  const [mode, setMode] = useState<ComposerMode>(initialMode);
  const busy = pending !== null;

  /** Opens a chat with the question; null when nothing was sent, so the
   * caller can put the words back in the box. */
  const ask = useCallback(
    async (asked: string): Promise<Chat | null> => {
      if (asked === "" || busy) {
        return null;
      }
      setPending("ask");
      try {
        return await createChat({ data: { question: asked } });
      } catch (error) {
        // The orchestrator's reason (--orchestrate off, a question already
        // running) is the whole message.
        toast.error(error instanceof Error ? error.message : String(error));
        return null;
      } finally {
        setPending(null);
      }
    },
    [busy, setPending]
  );

  const createTask = useCallback(() => setMode("task"), []);
  const backToChat = useCallback(() => setMode("chat"), []);

  return { ask, backToChat, createTask, mode };
}

/** What the agent did on the way to its answer, folded away the way a task's
 * tool activity is. */
function Worked({ steps, workedMs }: { steps: ChatStep[]; workedMs: number }) {
  return (
    <details className="group min-w-0">
      <summary className="cursor-pointer list-none rounded-md [&::-webkit-details-marker]:hidden">
        <Marker>
          <MarkerIcon>
            <ChevronIcon className="transition-transform group-open:rotate-90" />
          </MarkerIcon>
          <MarkerContent className="transition-colors group-hover:text-foreground">
            Worked for {shortSpan(workedMs)}
          </MarkerContent>
        </Marker>
      </summary>
      {steps.length > 0 ? (
        <ul className="mt-3 ml-2 flex min-w-0 flex-col gap-1.5 border-l pl-4 font-mono text-muted-foreground text-xs">
          {steps.map((step, index) => (
            // Position is stable: the trace is fixed once the answer arrives.
            // biome-ignore lint/suspicious/noArrayIndexKey: append-only trace
            <li key={index}>
              {step.tool}
              {step.detail ? ` ${step.detail}` : ""}
            </li>
          ))}
        </ul>
      ) : null}
    </details>
  );
}

/** The home conversation with the read-only workspace agent. Nothing here
 * changes state: `action` is the one way out, rendered under the latest
 * answer, and only the composer it hands off to queues work. */
export function WorkspaceChat({
  turns,
  pending,
  action,
  references,
}: {
  action: ReactNode;
  pending: boolean;
  references: Referenced;
  turns: ChatTurn[];
}) {
  const endRef = useRef<HTMLDivElement>(null);
  const lastAnswer = turns.filter((turn) => turn.role === "agent").at(-1);

  useEffect(() => {
    if (turns.length > 0 || pending) {
      endRef.current?.scrollIntoView({ block: "end" });
    }
  }, [turns.length, pending]);

  return (
    <div className="flex flex-1 flex-col">
      <ol aria-label="Conversation" className="flex flex-col gap-6">
        {turns.map((turn) => (
          <li className="min-w-0" key={`${turn.role}:${turn.at}`}>
            {turn.role === "person" ? (
              <Message align="end">
                <MessageContent>
                  <Bubble align="end" variant="muted">
                    <BubbleContent>
                      <p className="whitespace-pre-wrap [overflow-wrap:anywhere]">
                        {turn.text}
                      </p>
                    </BubbleContent>
                  </Bubble>
                </MessageContent>
              </Message>
            ) : (
              <Message>
                <MessageContent>
                  {turn.failed ||
                  (turn.worked_ms === 0 && turn.steps.length === 0) ? null : (
                    <Worked steps={turn.steps} workedMs={turn.worked_ms} />
                  )}
                  <Bubble variant="ghost">
                    <BubbleContent>
                      {turn.failed ? (
                        <p className="text-destructive">{turn.text}</p>
                      ) : (
                        <TextResponse>
                          <Markdown>{turn.text}</Markdown>
                        </TextResponse>
                      )}
                    </BubbleContent>
                  </Bubble>
                  {turn.failed ? null : (
                    <AnswerReferences
                      all={references}
                      text={[...turn.refs, turn.text].join(" ")}
                    />
                  )}
                  {turn === lastAnswer && !pending ? (
                    <MessageFooter>{action}</MessageFooter>
                  ) : null}
                </MessageContent>
              </Message>
            )}
          </li>
        ))}
        {pending ? (
          <li>
            <Marker>
              <MarkerContent>
                <ThinkingState label="Reading the workspace" />
              </MarkerContent>
            </Marker>
          </li>
        ) : null}
      </ol>
      <div className="scroll-mb-44" ref={endRef} />
    </div>
  );
}
