import { CaretRight } from "@phosphor-icons/react";
import type { ReactNode } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import { toast } from "sonner";

import { TextResponse } from "@/components/aicss/TextResponse";
import { ThinkingState } from "@/components/aicss/ThinkingState";
import type { Referenced } from "@/components/answer-references";
import { AnswerReferences } from "@/components/answer-references";
import { shortSpan } from "@/components/task-list";
import { Bubble, BubbleContent } from "@/components/ui/bubble";
import { Marker, MarkerContent, MarkerIcon } from "@/components/ui/marker";
import {
  Message,
  MessageContent,
  MessageFooter,
} from "@/components/ui/message";
import {
  type AskAnswer,
  type AskStep,
  askWorkspace,
  enhancePrompt,
} from "@/lib/lgtm/server";

export interface ChatTurn {
  id: string;
  /** Only an assistant turn carries these. */
  refs?: string[];
  role: "user" | "assistant";
  steps?: AskStep[];
  text: string;
  workedMs?: number;
}

export type ComposerMode = "chat" | "task";

/** The conversation as the models read it: the agent answering a follow-up
 * and the one writing a task brief both get the same text. */
function transcript(turns: ChatTurn[]): string {
  return turns
    .map((item) => `${item.role === "user" ? "Person" : "Agent"}: ${item.text}`)
    .join("\n\n");
}

// ponytail: the whole transcript rides along on every question; trim to the
// last few turns when answers get long.
function askedQuestion(
  turns: ChatTurn[],
  question: string,
  repository: string
): string {
  const scope = repository ? `Repository: ${repository}\n\n` : "";
  if (turns.length === 0) {
    return `${scope}${question}`;
  }
  return `${scope}Earlier in this conversation:\n\n${transcript(turns)}\n\nNow the person asks:\n${question}`;
}

function chatTurn(role: ChatTurn["role"], text: string): ChatTurn {
  return { id: crypto.randomUUID(), role, text };
}

function answerTurn(answer: AskAnswer): ChatTurn {
  return {
    id: crypto.randomUUID(),
    refs: answer.refs,
    role: "assistant",
    steps: answer.steps,
    text: answer.answer,
    workedMs: answer.worked_ms,
  };
}

/** Chat is the default: reading the workspace costs nothing. Task mode is the
 * explicit step that can queue work, so it is the one a person has to pick.
 * `pending` is shared with the composer's own actions so one thing runs at a
 * time. */
export function useWorkspaceChat({
  repository,
  pending,
  setPending,
}: {
  pending: string | null;
  repository: string;
  setPending: (next: "ask" | "brief" | null) => void;
}) {
  const [mode, setMode] = useState<ComposerMode>("chat");
  const [turns, setTurns] = useState<ChatTurn[]>([]);
  const busy = pending !== null;

  /** Resolves to whether the question was answered; the caller puts an
   * unanswered one back in the box. */
  const ask = useCallback(
    async (asked: string): Promise<boolean> => {
      if (asked === "" || busy) {
        return true;
      }
      const before = turns;
      setTurns([...before, chatTurn("user", asked)]);
      setPending("ask");
      try {
        const answer = await askWorkspace({
          data: { question: askedQuestion(before, asked, repository) },
        });
        setTurns((current) => [...current, answerTurn(answer)]);
        return true;
      } catch (error) {
        // The orchestrator's reason (--orchestrate off, a question already
        // running) is the whole message.
        toast.error(error instanceof Error ? error.message : String(error));
        setTurns(before);
        return false;
      } finally {
        setPending(null);
      }
    },
    [busy, repository, setPending, turns]
  );

  /** The step from talking to doing. Resolves to the task draft: a brief
   * written from the transcript, or null when there is no conversation and
   * whatever is in the box already is the draft. */
  const createTask = useCallback(async (): Promise<string | null> => {
    if (busy) {
      return null;
    }
    setMode("task");
    if (turns.length === 0) {
      return null;
    }
    setPending("brief");
    try {
      const result = await enhancePrompt({
        data: {
          prompt: `Conversation with the workspace agent:\n\n${transcript(turns)}`,
          repository: repository || undefined,
        },
      });
      return result.prompt;
    } catch (error) {
      // Nothing could write the brief: the last thing the person said stands
      // in for it, and the reason says what was missing.
      toast.error(error instanceof Error ? error.message : String(error));
      return turns.filter((item) => item.role === "user").at(-1)?.text ?? "";
    } finally {
      setPending(null);
    }
  }, [busy, repository, setPending, turns]);

  const backToChat = useCallback(() => setMode("chat"), []);

  return { ask, backToChat, createTask, mode, turns };
}

/** What the agent did on the way to its answer, folded away the way a task's
 * tool activity is. */
function Worked({ steps, workedMs }: { steps: AskStep[]; workedMs: number }) {
  return (
    <details className="group min-w-0">
      <summary className="cursor-pointer list-none rounded-md [&::-webkit-details-marker]:hidden">
        <Marker>
          <MarkerIcon>
            <CaretRight className="transition-transform group-open:rotate-90" />
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
  const lastAnswer = turns.filter((turn) => turn.role === "assistant").at(-1);

  useEffect(() => {
    if (turns.length > 0 || pending) {
      endRef.current?.scrollIntoView({ block: "end" });
    }
  }, [turns.length, pending]);

  return (
    <div className="flex flex-1 flex-col">
      <ol aria-label="Conversation" className="flex flex-col gap-6">
        {turns.map((turn) => (
          <li key={turn.id}>
            {turn.role === "user" ? (
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
                  {turn.workedMs === undefined ? null : (
                    <Worked steps={turn.steps ?? []} workedMs={turn.workedMs} />
                  )}
                  <Bubble variant="ghost">
                    <BubbleContent>
                      <TextResponse>
                        <Markdown>{turn.text}</Markdown>
                      </TextResponse>
                    </BubbleContent>
                  </Bubble>
                  <AnswerReferences
                    all={references}
                    text={[...(turn.refs ?? []), turn.text].join(" ")}
                  />
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
