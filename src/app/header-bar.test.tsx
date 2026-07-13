import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { RepositoryInfo } from "@/types/git";
import { HeaderBar } from "./header-bar";

afterEach(cleanup);

const HEAD_TRIGGER = /main/;
const LOCAL_BRANCH = /feature/;
const REMOTE_BRANCH = /origin\/main/;

const INFO: RepositoryInfo = {
  rootPath: "/repo",
  displayName: "repo",
  currentBranch: "main",
  headSha: "abc1234",
  detached: false,
  unborn: false,
  remoteUrl: null,
  defaultBaseBranch: "main",
  branches: ["main", "feature"],
  remoteBranches: ["origin/main"],
};

function renderHeader() {
  const onModeChange = vi.fn();
  render(
    <TooltipProvider>
      <HeaderBar
        info={INFO}
        mode={{ kind: "working-tree" }}
        onClose={vi.fn()}
        onModeChange={onModeChange}
        onOpenSettings={vi.fn()}
        onViewChange={vi.fn()}
        view="split"
      />
    </TooltipProvider>
  );
  return { onModeChange };
}

// Regression: the "Review branch" GroupLabel outside a RadioGroup made Base UI
// throw on open, blanking the whole app.
it("opens the branch switcher and lists local and remote branches", async () => {
  renderHeader();

  await userEvent.click(screen.getByRole("button", { name: HEAD_TRIGGER }));

  expect(screen.getByText("Review branch")).toBeInTheDocument();
  expect(
    screen.getByRole("menuitemradio", { name: LOCAL_BRANCH })
  ).toBeInTheDocument();
  expect(
    screen.getByRole("menuitemradio", { name: REMOTE_BRANCH })
  ).toBeInTheDocument();
});

it("switches the reviewed head without touching working-tree base", async () => {
  const { onModeChange } = renderHeader();

  await userEvent.click(screen.getByRole("button", { name: HEAD_TRIGGER }));
  await userEvent.click(
    screen.getByRole("menuitemradio", { name: LOCAL_BRANCH })
  );

  expect(onModeChange).toHaveBeenCalledWith({
    kind: "branch",
    base: "main",
    head: "feature",
  });
});
