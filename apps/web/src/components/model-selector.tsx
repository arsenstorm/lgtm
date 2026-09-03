import { Combobox } from "@base-ui/react/combobox";
import { PreviewCard } from "@base-ui/react/preview-card";
import { CaretDown, Lightning, MagnifyingGlass } from "@phosphor-icons/react";
import type { ChangeEvent } from "react";
import { useCallback, useMemo, useRef, useState } from "react";

import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import anthropicLogo from "@/icons/anthropic-logo.svg";
import openAiBlackLogo from "@/icons/openai-logo-black.svg";
import openAiWhiteLogo from "@/icons/openai-logo-white.svg";
import type { Executor, ReasoningEffort } from "@/lib/lgtm/types";
import { cn } from "@/lib/utils";

export interface LlmModel {
  contextTokens: number;
  description: string;
  executor: Executor;
  inputPrice: number;
  intelligence: number;
  label: string;
  model: string;
  outputPrice: number;
  outputTokensPerSecond: number;
  provider: "Anthropic" | "OpenAI";
  value: string;
}

// https://artificialanalysis.ai/leaderboards/models and its linked model
// pages, retrieved 2026-09-03. Scores and speed are max-effort results.
export const MODELS: readonly LlmModel[] = [
  {
    contextTokens: 1_000_000,
    description:
      "Anthropic’s strongest long-horizon agent model, with adaptive reasoning and Opus fallback.",
    executor: "claude",
    inputPrice: 10,
    intelligence: 66,
    label: "Fable 5.1",
    model: "claude-fable-5-1",
    outputPrice: 50,
    outputTokensPerSecond: 284,
    provider: "Anthropic",
    value: "claude-fable-5-1",
  },
  {
    contextTokens: 1_000_000,
    description:
      "Anthropic’s flagship for deep analysis, difficult coding, and sustained agentic work.",
    executor: "claude",
    inputPrice: 5,
    intelligence: 63,
    label: "Opus 5",
    model: "claude-opus-5",
    outputPrice: 25,
    outputTokensPerSecond: 57,
    provider: "Anthropic",
    value: "claude-opus-5",
  },
  {
    contextTokens: 1_000_000,
    description:
      "A balanced Claude model for everyday coding with strong reasoning at lower cost.",
    executor: "claude",
    inputPrice: 2,
    intelligence: 55,
    label: "Sonnet 5",
    model: "claude-sonnet-5",
    outputPrice: 10,
    outputTokensPerSecond: 71,
    provider: "Anthropic",
    value: "claude-sonnet-5",
  },
  {
    contextTokens: 1_000_000,
    description:
      "OpenAI’s strongest GPT-5.6 model for detailed, polished, high-value coding work.",
    executor: "codex",
    inputPrice: 4,
    intelligence: 61,
    label: "5.6 Sol",
    model: "gpt-5.6-sol",
    outputPrice: 20,
    outputTokensPerSecond: 99,
    provider: "OpenAI",
    value: "gpt-5.6-sol",
  },
  {
    contextTokens: 1_000_000,
    description:
      "OpenAI’s everyday GPT-5.6 workhorse, balancing capability, throughput, and cost.",
    executor: "codex",
    inputPrice: 2,
    intelligence: 57,
    label: "5.6 Terra",
    model: "gpt-5.6-terra",
    outputPrice: 12,
    outputTokensPerSecond: 98,
    provider: "OpenAI",
    value: "gpt-5.6-terra",
  },
  {
    contextTokens: 1_000_000,
    description:
      "OpenAI’s fast, economical GPT-5.6 model for clear and repeatable coding tasks.",
    executor: "codex",
    inputPrice: 0.2,
    intelligence: 52,
    label: "5.6 Luna",
    model: "gpt-5.6-luna",
    outputPrice: 1.2,
    outputTokensPerSecond: 120,
    provider: "OpenAI",
    value: "gpt-5.6-luna",
  },
] as const;

interface ModelSelectorProps {
  configurations: Record<string, ReasoningEffort>;
  disabled?: boolean;
  model: LlmModel;
  onConfigurationChange: (model: string, effort: ReasoningEffort) => void;
  onModelChange: (model: LlmModel) => void;
}

const REASONING_LEVELS: readonly ReasoningEffort[] = ["low", "medium", "high"];
const BAR_SEGMENTS = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"];

const MAX_INTELLIGENCE = Math.max(...MODELS.map((model) => model.intelligence));
const MAX_SPEED = Math.max(
  ...MODELS.map((model) => model.outputTokensPerSecond)
);
const MAX_CONTEXT = Math.max(...MODELS.map((model) => model.contextTokens));
const MAX_COMBINED_PRICE = Math.max(
  ...MODELS.map((model) => model.inputPrice + model.outputPrice)
);

function score(value: number, maximum: number): number {
  return Math.max(1, Math.round((value / maximum) * 10));
}

function costScore(model: LlmModel): number {
  const combined = model.inputPrice + model.outputPrice;
  return Math.max(1, 11 - score(combined, MAX_COMBINED_PRICE));
}

function isSameModel(item: LlmModel, value: LlmModel): boolean {
  return item.value === value.value;
}

function metricSegmentClass(
  provider: LlmModel["provider"],
  filled: boolean
): string {
  if (!filled) {
    return "bg-foreground/10";
  }
  return provider === "Anthropic" ? "bg-[#D97757]" : "bg-[#10A37F]";
}

function ProviderMark({
  className,
  provider,
}: {
  className?: string;
  provider: LlmModel["provider"];
}) {
  const shellClassName =
    provider === "Anthropic"
      ? "bg-[#D97757]/12 text-[#C6613F] dark:bg-[#D97757]/18 dark:text-[#E59679]"
      : "bg-[#10A37F]/12 text-[#087F63] dark:bg-[#10A37F]/18 dark:text-[#39C8A0]";

  if (provider === "Anthropic") {
    return (
      <span
        className={cn(
          "grid size-5 shrink-0 place-items-center rounded-md",
          shellClassName,
          className
        )}
      >
        <img
          alt=""
          aria-hidden="true"
          className="size-3.5"
          height={14}
          src={anthropicLogo}
          width={14}
        />
      </span>
    );
  }
  return (
    <span
      className={cn(
        "grid size-5 shrink-0 place-items-center rounded-md",
        shellClassName,
        className
      )}
    >
      <img
        alt=""
        aria-hidden="true"
        className="size-3.5 dark:hidden"
        height={14}
        src={openAiBlackLogo}
        width={14}
      />
      <img
        alt=""
        aria-hidden="true"
        className="hidden size-3.5 dark:block"
        height={14}
        src={openAiWhiteLogo}
        width={14}
      />
    </span>
  );
}

function MetricBar({
  detail,
  label,
  provider,
  value,
}: {
  detail: string;
  label: string;
  provider: LlmModel["provider"];
  value: number;
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <div
            aria-label={`${label}: ${detail}`}
            className="flex min-w-0 flex-col gap-1"
            role="img"
          />
        }
      >
        <span className="text-muted-foreground text-xs">{label}</span>
        <span className="grid grid-cols-10 gap-0.5">
          {BAR_SEGMENTS.map((segment, index) => (
            <span
              className={cn(
                "h-1 rounded-full",
                metricSegmentClass(provider, index < value)
              )}
              key={segment}
            />
          ))}
        </span>
      </TooltipTrigger>
      <TooltipContent>{detail}</TooltipContent>
    </Tooltip>
  );
}

function ReasoningBadge({ effort }: { effort: ReasoningEffort }) {
  return (
    <span className="shrink-0 text-muted-foreground/65 text-sm capitalize">
      {effort}
    </span>
  );
}

function ModelPreview({
  effort,
  model,
  onConfigurationChange,
}: {
  effort: ReasoningEffort;
  model: LlmModel;
  onConfigurationChange: (model: string, effort: ReasoningEffort) => void;
}) {
  const handleReasoningChange = useCallback(
    (event: ChangeEvent<HTMLInputElement>) => {
      const level = REASONING_LEVELS[Number(event.currentTarget.value)];
      if (level) {
        onConfigurationChange(model.value, level);
      }
    },
    [model.value, onConfigurationChange]
  );
  const effortIndex = REASONING_LEVELS.indexOf(effort);

  return (
    <div className="w-64 divide-y divide-border/60">
      <div className="flex flex-col gap-3 p-3">
        <div className="flex flex-col gap-1">
          <p className="font-medium text-sm">{model.label}</p>
          <span className="flex items-center gap-1.5 text-muted-foreground text-xs">
            <ProviderMark provider={model.provider} />
            {model.provider}
          </span>
        </div>
        <p className="text-pretty text-muted-foreground text-xs leading-4">
          {model.description}
        </p>
        <TooltipProvider>
          <div className="grid grid-cols-2 gap-3">
            <MetricBar
              detail={`${model.intelligence} Artificial Analysis Intelligence Index at max effort`}
              label="Intelligence"
              provider={model.provider}
              value={score(model.intelligence, MAX_INTELLIGENCE)}
            />
            <MetricBar
              detail={`${model.outputTokensPerSecond} output tokens/second at max effort`}
              label="Speed"
              provider={model.provider}
              value={score(model.outputTokensPerSecond, MAX_SPEED)}
            />
            <MetricBar
              detail={`${model.contextTokens.toLocaleString()} token context window`}
              label="Context"
              provider={model.provider}
              value={score(model.contextTokens, MAX_CONTEXT)}
            />
            <MetricBar
              detail={`$${model.inputPrice.toFixed(2)} input · $${model.outputPrice.toFixed(2)} output per 1M tokens`}
              label="Cost"
              provider={model.provider}
              value={costScore(model)}
            />
          </div>
        </TooltipProvider>
      </div>
      <fieldset className="flex flex-col p-3">
        <div className="flex items-center gap-1.5">
          <Lightning
            aria-hidden="true"
            className="size-3.5 text-muted-foreground"
          />
          <legend className="font-medium text-xs">Reasoning</legend>
          <span className="ml-auto text-muted-foreground text-xs capitalize">
            {effort}
          </span>
        </div>
        <div
          className={cn(
            "reasoning-slider-shell relative",
            model.provider === "Anthropic"
              ? "reasoning-slider-anthropic"
              : "reasoning-slider-openai"
          )}
          data-effort-index={effortIndex}
        >
          <input
            aria-label="Reasoning effort"
            className="reasoning-slider absolute inset-0 z-10 opacity-0"
            max={REASONING_LEVELS.length - 1}
            min={0}
            name={`reasoning-${model.value}`}
            onChange={handleReasoningChange}
            step={1}
            type="range"
            value={effortIndex}
          />
          <div
            aria-hidden="true"
            className="reasoning-slider-rail absolute inset-x-3.5 top-1/2 overflow-hidden rounded-full"
          >
            <span className="reasoning-slider-points absolute inset-0 flex items-center justify-between px-0.5">
              {REASONING_LEVELS.map((level) => (
                <span
                  className="reasoning-slider-point size-1 rounded-full"
                  key={level}
                />
              ))}
            </span>
          </div>
          <span
            aria-hidden="true"
            className="reasoning-slider-thumb-position absolute top-1/2 rounded-full"
          >
            <span className="reasoning-slider-thumb block size-full rounded-full" />
          </span>
        </div>
      </fieldset>
    </div>
  );
}

function ModelItem({
  effort,
  model,
  preview,
}: {
  effort: ReasoningEffort;
  model: LlmModel;
  preview: PreviewCard.Handle<LlmModel>;
}) {
  return (
    <Combobox.Item
      className={cn(
        "group rounded-md p-0 text-sm outline-none",
        model.provider === "Anthropic"
          ? "data-highlighted:bg-[#D97757]/10 data-selected:bg-[#D97757]/10"
          : "data-highlighted:bg-[#10A37F]/10 data-selected:bg-[#10A37F]/10"
      )}
      value={model}
    >
      <PreviewCard.Trigger
        closeDelay={180}
        delay={80}
        handle={preview}
        payload={model}
        render={
          <div className="flex min-w-0 items-start gap-2 px-2 py-1.5 outline-none" />
        }
      >
        <ProviderMark className="mt-0.5" provider={model.provider} />
        <span className="flex min-w-0 flex-1 flex-col gap-0.5">
          <span className="truncate">{model.label}</span>
          <span className="text-muted-foreground text-xs">
            {model.provider}
          </span>
        </span>
        <ReasoningBadge effort={effort} />
      </PreviewCard.Trigger>
    </Combobox.Item>
  );
}

export function ModelSelector({
  configurations,
  disabled = false,
  model,
  onConfigurationChange,
  onModelChange,
}: ModelSelectorProps) {
  const listRef = useRef<HTMLDivElement>(null);
  const [showFade, setShowFade] = useState(false);
  const preview = useMemo(() => PreviewCard.createHandle<LlmModel>(), []);

  const closePreview = useCallback(() => preview.close(), [preview]);
  const updateFade = useCallback(() => {
    const scrollHeight = listRef.current?.scrollHeight ?? 0;
    const scrollTop = listRef.current?.scrollTop ?? 0;
    const clientHeight = listRef.current?.clientHeight ?? 0;
    setShowFade(scrollHeight - scrollTop - clientHeight > 4);
  }, []);
  const handleOpenChange = useCallback(
    (open: boolean) => {
      if (open) {
        requestAnimationFrame(updateFade);
      } else {
        closePreview();
      }
    },
    [closePreview, updateFade]
  );
  const handleInputValueChange = useCallback(() => {
    closePreview();
    requestAnimationFrame(updateFade);
  }, [closePreview, updateFade]);
  const handleValueChange = useCallback(
    (value: LlmModel | null) => {
      if (value) {
        onModelChange(value);
      }
    },
    [onModelChange]
  );

  return (
    <Combobox.Root<LlmModel>
      autoHighlight
      isItemEqualToValue={isSameModel}
      items={MODELS}
      onInputValueChange={handleInputValueChange}
      onOpenChange={handleOpenChange}
      onValueChange={handleValueChange}
      value={model}
    >
      <Combobox.Trigger
        aria-label="Select model"
        className="flex min-w-0 max-w-44 items-center gap-1.5 rounded-sm font-medium text-foreground text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:opacity-40"
        disabled={disabled}
      >
        <Combobox.Value>
          {(value: LlmModel) => (
            <span className="min-w-0 truncate">{value.label}</span>
          )}
        </Combobox.Value>
        <ReasoningBadge effort={configurations[model.value] ?? "medium"} />
        <CaretDown
          aria-hidden="true"
          className="size-3 shrink-0 text-muted-foreground"
        />
      </Combobox.Trigger>

      <Combobox.Portal>
        <Combobox.Positioner align="end" className="z-50" sideOffset={6}>
          <Combobox.Popup className="w-[min(18rem,var(--available-width))] overflow-hidden rounded-xl border bg-popover text-popover-foreground shadow-md outline-none">
            <PreviewCard.Root<LlmModel> handle={preview}>
              {({ payload }) => (
                <>
                  <Combobox.InputGroup className="flex items-center gap-2 border-b px-2">
                    <MagnifyingGlass
                      aria-hidden="true"
                      className="size-4 shrink-0 text-muted-foreground"
                    />
                    <Combobox.Input
                      aria-label="Search models"
                      autoComplete="off"
                      className="h-10 min-w-0 flex-1 bg-transparent text-base outline-none placeholder:text-muted-foreground sm:text-sm"
                      name="model-search"
                      onFocus={closePreview}
                      placeholder="Search models…"
                      spellCheck={false}
                    />
                  </Combobox.InputGroup>
                  <Combobox.Empty>
                    <div className="px-3 py-6 text-center text-muted-foreground text-sm">
                      No models found
                    </div>
                  </Combobox.Empty>
                  <div className="relative">
                    <Combobox.List
                      className="max-h-[min(20rem,var(--available-height))] space-y-0.5 overflow-y-auto p-1"
                      onScroll={updateFade}
                      ref={listRef}
                    >
                      {(item: LlmModel) => (
                        <ModelItem
                          effort={configurations[item.value] ?? "medium"}
                          key={item.value}
                          model={item}
                          preview={preview}
                        />
                      )}
                    </Combobox.List>
                    <div
                      aria-hidden="true"
                      className={cn(
                        "pointer-events-none absolute inset-x-0 bottom-0 h-8 bg-linear-to-t from-popover to-transparent",
                        showFade ? "opacity-100" : "opacity-0"
                      )}
                    />
                  </div>
                  <PreviewCard.Portal keepMounted>
                    <PreviewCard.Positioner
                      align="center"
                      className="z-60 hidden sm:block"
                      side="left"
                      sideOffset={8}
                    >
                      <PreviewCard.Popup className="overflow-hidden rounded-xl border bg-popover text-popover-foreground shadow-md outline-none">
                        {payload ? (
                          <ModelPreview
                            effort={configurations[payload.value] ?? "medium"}
                            model={payload}
                            onConfigurationChange={onConfigurationChange}
                          />
                        ) : null}
                      </PreviewCard.Popup>
                    </PreviewCard.Positioner>
                  </PreviewCard.Portal>
                </>
              )}
            </PreviewCard.Root>
          </Combobox.Popup>
        </Combobox.Positioner>
      </Combobox.Portal>
    </Combobox.Root>
  );
}
