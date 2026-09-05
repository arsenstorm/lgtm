import type { ErrorComponentProps } from "@tanstack/react-router";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { toast } from "sonner";

import { LoaderIcon, PlusIcon } from "@/components/icons";
import { ListGroup } from "@/components/list-group";
import { OrchestratorError } from "@/components/orchestrator-error";
import { PageHeading } from "@/components/page-heading";
import { TimeAgo } from "@/components/time-ago";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { createSkill, getSkills } from "@/lib/lgtm/server";
import type { Skill } from "@/lib/lgtm/types";
import { groupByRepository } from "@/routes/memories";

export const Route = createFileRoute("/skills")({
  loader: async () => ({ skills: await getSkills() }),
  component: SkillsPage,
  errorComponent: SkillsError,
});

/** The frontmatter the orchestrator insists on, with a name stamped from the
 *  browser's clock so a page of fresh skills still reads in order; the spec
 *  allows only lowercase letters, digits and single hyphens. */
function template(now = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  const stamp = `${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}`;
  return `---
name: new-skill-${stamp}
description: What this does and when an agent should use it.
---

`;
}

const byEdited = (a: Skill, b: Skill) => b.updated_at - a.updated_at;

function SkillsPage() {
  const { skills } = Route.useLoaderData();
  const navigate = useNavigate();
  const [creating, setCreating] = useState(false);

  const groups = groupByRepository(skills, byEdited);

  async function create() {
    setCreating(true);
    try {
      const skill = await createSkill({
        data: { content: template(), repository: null },
      });
      // The blank document opening is the success signal; a toast on top of it
      // would only say what the screen already shows.
      await navigate({ to: "/skills/$id", params: { id: skill.id } });
    } catch (error) {
      // The orchestrator's refusal reason is the whole message; genericising it
      // would throw away the only thing that says what to do next.
      toast.error(error instanceof Error ? error.message : String(error));
    } finally {
      setCreating(false);
    }
  }

  return (
    // The shell's <main> is an unpadded scroll container, so the page owns its
    // own gutters.
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-6 px-4 py-6 sm:px-6 lg:px-8">
      <PageHeading meta={skills.length} title="Skills">
        <Button disabled={creating} onClick={create} size="lg">
          {creating ? (
            <LoaderIcon
              className="motion-safe:animate-spin"
              data-icon="inline-start"
            />
          ) : (
            <PlusIcon data-icon="inline-start" />
          )}
          New skill
        </Button>
      </PageHeading>

      {skills.length === 0 ? (
        <p className="text-muted-foreground text-sm">
          No skills yet. New skill starts one, and{" "}
          <code>lgtm skill import</code> brings in a directory of them.
        </p>
      ) : (
        <div className="flex flex-col gap-1">
          {groups.map((entry) => (
            <SkillGroup
              key={entry.key}
              label={entry.label}
              skills={entry.items}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function SkillGroup({ label, skills }: { label: string; skills: Skill[] }) {
  return (
    <ListGroup count={skills.length} label={label}>
      <ul className="flex flex-col py-1">
        {skills.map((skill) => (
          <li key={skill.id}>
            <SkillRow skill={skill} />
          </li>
        ))}
      </ul>
    </ListGroup>
  );
}

function SkillRow({ skill }: { skill: Skill }) {
  return (
    <Link
      className="flex items-center gap-3 rounded-md py-2.5 pr-2 pl-7 text-sm hover:bg-foreground/4"
      params={{ id: skill.id }}
      to="/skills/$id"
    >
      <span className="min-w-0 truncate">{skill.name}</span>
      <span className="min-w-0 flex-1 truncate text-muted-foreground">
        {skill.description}
      </span>

      {/* Approved is the boring default; only a proposal needs saying. */}
      {skill.verification === "agent_proposed" && (
        <Badge
          className="border-amber-600/30 text-amber-700 dark:text-amber-400"
          variant="outline"
        >
          proposed
        </Badge>
      )}

      <TimeAgo
        at={skill.updated_at}
        className="w-16 shrink-0 truncate text-end text-muted-foreground tabular-nums"
      />
    </Link>
  );
}

function SkillsError(props: ErrorComponentProps) {
  return <OrchestratorError what="skills" {...props} />;
}
