import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";

import { ActionIcon } from "@/components/action-icon";
import { CheckIcon, PencilIcon, PlusIcon, TrashIcon } from "@/components/icons";
import { OrchestratorError } from "@/components/orchestrator-error";
import { PageHeading } from "@/components/page-heading";
import { SELECT_CLASS } from "@/components/task-composer";
import { TimeAgo } from "@/components/time-ago";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { useAction } from "@/hooks/use-action";
import { ARMED_CLASS, useArmedConfirm } from "@/hooks/use-armed-confirm";
import {
  approveSkill,
  createSkill,
  deleteSkill,
  getProjects,
  getSkills,
  updateSkill,
} from "@/lib/lgtm/server";
import type { Project, Skill } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";
import { groupByRepository } from "@/routes/memories";

export const Route = createFileRoute("/skills")({
  loader: async () => {
    const [skills, projects] = await Promise.all([getSkills(), getProjects()]);
    return { projects, skills };
  },
  component: SkillsPage,
  errorComponent: SkillsError,
});

const SKILL_EDITOR_CLASS = "min-h-64 font-mono text-xs";

/** The frontmatter the orchestrator insists on, with the two fields already
 *  named so a new skill only has to be filled in. */
const TEMPLATE = `---
name: my-skill
description: What this does and when an agent should use it.
---

`;

function SkillsPage() {
  const { projects, skills } = Route.useLoaderData();
  const [composing, setComposing] = useState(false);
  const groups = groupByRepository(skills, (a, b) =>
    a.name.localeCompare(b.name)
  );

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-8 px-4 py-6 sm:px-6 lg:px-8">
      <PageHeading meta={skills.length} title="Skills">
        <Button onClick={() => setComposing(true)} size="sm" variant="outline">
          <PlusIcon data-icon="inline-start" />
          New skill
        </Button>
      </PageHeading>

      {composing && (
        <NewSkill onClose={() => setComposing(false)} projects={projects} />
      )}

      {groups.length === 0 ? (
        <p className="text-muted-foreground text-sm">No skills yet.</p>
      ) : (
        groups.map((entry) => (
          <section className="flex flex-col gap-2" key={entry.key}>
            <h2 className="truncate font-medium text-muted-foreground text-sm">
              {entry.label}
            </h2>
            <ul className="-mx-2 divide-y divide-foreground/5" role="list">
              {entry.items.map((skill) => (
                <li key={skill.id}>
                  <SkillRow skill={skill} />
                </li>
              ))}
            </ul>
          </section>
        ))
      )}
    </div>
  );
}

function NewSkill({
  onClose,
  projects,
}: {
  onClose: () => void;
  projects: Project[];
}) {
  const [content, setContent] = useState(TEMPLATE);
  const [repository, setRepository] = useState("");
  const { pending, busy, run } = useAction<"create">();

  const written = content.trim();

  async function create() {
    if (!written) {
      return;
    }
    const made = await run(
      "create",
      () =>
        createSkill({
          data: { content, repository: repository || null },
        }),
      "Skill created"
    );
    if (made) {
      onClose();
    }
  }

  return (
    <div className="flex flex-col gap-2">
      <Textarea
        aria-label="SKILL.md"
        autoFocus
        className={SKILL_EDITOR_CLASS}
        disabled={busy}
        onChange={(event) => setContent(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            onClose();
          }
        }}
        value={content}
      />
      <div className="flex items-center gap-2">
        <select
          aria-label="Repository"
          className={cn(SELECT_CLASS, "w-56")}
          disabled={busy}
          onChange={(event) => setRepository(event.target.value)}
          value={repository}
        >
          <option value="">Every repository</option>
          {projects
            .filter((project) => project.repository)
            .map((project) => (
              <option key={project.id} value={project.repository ?? ""}>
                {project.name}
              </option>
            ))}
        </select>
        <Button disabled={busy || !written} onClick={create} size="sm">
          <ActionIcon busy={pending === "create"} icon={CheckIcon} />
          Create
        </Button>
        <Button disabled={busy} onClick={onClose} size="sm" variant="ghost">
          Cancel
        </Button>
      </div>
    </div>
  );
}

type Action = "save" | "delete" | "approve";

function SkillRow({ skill }: { skill: Skill }) {
  // null means "not editing" — an empty draft is a distinct, valid state.
  const [draft, setDraft] = useState<string | null>(null);
  const { armed, arm, disarm, ref: deleteRef } = useArmedConfirm();
  const { pending, busy, run } = useAction<Action>({ onStart: disarm });

  const proposed = skill.verification === "agent_proposed";

  const edited = (draft ?? "").trim();

  async function save() {
    if (!edited || edited === skill.content) {
      return;
    }
    const saved = await run(
      "save",
      () => updateSkill({ data: { id: skill.id, content: edited } }),
      // The orchestrator treats an edit as sign-off, so say so.
      proposed ? "Skill updated and approved" : "Skill updated"
    );
    if (saved) {
      setDraft(null);
    }
  }

  if (draft !== null) {
    return (
      <div className="flex items-start gap-3 px-2 py-2.5 text-sm">
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <Textarea
            aria-label="SKILL.md"
            autoFocus
            className={SKILL_EDITOR_CLASS}
            disabled={busy}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                setDraft(null);
              }
            }}
            value={draft}
          />
          <div className="flex items-center gap-2">
            <Button
              disabled={busy || !edited || edited === skill.content}
              onClick={save}
              size="sm"
            >
              <ActionIcon busy={pending === "save"} icon={CheckIcon} />
              Save
            </Button>
            <Button
              disabled={busy}
              onClick={() => setDraft(null)}
              size="sm"
              variant="ghost"
            >
              Cancel
            </Button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="group/row flex items-start gap-3 px-2 py-2.5 text-sm">
      <span className="font-medium">{skill.name}</span>
      <span
        className="min-w-0 flex-1 truncate text-muted-foreground"
        title={skill.description}
      >
        {skill.description}
      </span>

      {/* Always in the flow so revealing them cannot reflow the row. */}
      <div className="flex shrink-0 items-center gap-0.5 opacity-0 pointer-coarse:opacity-100 transition-opacity group-focus-within/row:opacity-100 group-hover/row:opacity-100">
        {proposed && (
          <Button
            aria-label="Approve skill"
            className="text-muted-foreground"
            disabled={busy}
            onClick={() =>
              run(
                "approve",
                () => approveSkill({ data: skill.id }),
                "Skill approved"
              )
            }
            size="icon-sm"
            variant="ghost"
          >
            <ActionIcon busy={pending === "approve"} icon={CheckIcon} />
          </Button>
        )}
        <Button
          aria-label="Edit skill"
          className="text-muted-foreground"
          disabled={busy}
          onClick={() => setDraft(skill.content)}
          size="icon-sm"
          variant="ghost"
        >
          <PencilIcon />
        </Button>
        <Button
          aria-label={armed ? "Confirm delete skill" : "Delete skill"}
          className={cn(armed ? ARMED_CLASS : "text-muted-foreground")}
          disabled={busy}
          onClick={() =>
            armed
              ? run(
                  "delete",
                  () => deleteSkill({ data: skill.id }),
                  "Skill deleted"
                )
              : arm()
          }
          ref={deleteRef}
          size={armed ? "sm" : "icon-sm"}
          variant={armed ? "destructive" : "ghost"}
        >
          <ActionIcon busy={pending === "delete"} icon={TrashIcon} />
          {armed && "Confirm delete"}
        </Button>
      </div>

      {/* Approved is the boring default; only a proposal needs saying. */}
      {proposed && (
        <Badge
          className="border-amber-600/30 text-amber-700 dark:text-amber-400"
          variant="outline"
        >
          proposed
        </Badge>
      )}

      {skill.source === "agent" && (
        <span className="shrink-0 text-muted-foreground text-xs">agent</span>
      )}

      {skill.files.length > 0 && (
        <span className="shrink-0 text-muted-foreground text-xs tabular-nums">
          {skill.files.length} {skill.files.length === 1 ? "file" : "files"}
        </span>
      )}

      <span className="shrink-0 text-muted-foreground text-xs tabular-nums">
        v{skill.revision}
      </span>

      <TimeAgo
        at={skill.updated_at}
        className="w-16 shrink-0 truncate text-end text-muted-foreground tabular-nums"
      />
    </div>
  );
}

function SkillsError(props: ErrorComponentProps) {
  return <OrchestratorError what="skills" {...props} />;
}
