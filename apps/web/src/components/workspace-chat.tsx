import type { ReactNode } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import { toast } from "sonner";

import { TextResponse } from "@/components/aicss/TextResponse";
import { ThinkingState } from "@/components/aicss/ThinkingState";
import { Bubble, BubbleContent } from "@/components/ui/bubble";
import { Marker, MarkerContent } from "@/components/ui/marker";
import {
  Message,
  MessageContent,
  MessageFooter,
} from "@/components/ui/message";
import { askWorkspace, enhancePrompt } from "@/lib/lgtm/server";

export interface ChatTurn {
  id: string;
  role: "user" | "assistant";
  text: string;
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
        const { answer } = await askWorkspace({
          data: { question: askedQuestion(before, asked, repository) },
        });
        setTurns((current) => [...current, chatTurn("assistant", answer)]);
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

/** The home conversation with the read-only workspace agent. Nothing here
 * changes state: `action` is the one way out, rendered under the latest
 * answer, and only the composer it hands off to queues work. */
export function WorkspaceChat({
  turns,
  pending,
  action,
}: {
  action: ReactNode;
  pending: boolean;
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
    <div className="flex flex-1 flex-col justify-end">
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
                  <Bubble variant="ghost">
                    <BubbleContent>
                      <TextResponse>
                        <Markdown>{turn.text}</Markdown>
                      </TextResponse>
                    </BubbleContent>
                  </Bubble>
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
      <div ref={endRef} />
    </div>
  );
}
