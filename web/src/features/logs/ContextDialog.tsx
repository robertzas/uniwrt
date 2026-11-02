import { ChevronsUpDown } from "lucide";
import { createEffect, createMemo, createSignal, For, Show } from "solid-js";
import type { LogPattern } from "../../api/gen/LogPattern";
import type { LogRecord } from "../../api/gen/LogRecord";
import { api } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { LogRow, type SourceLabel } from "./LogRow";

const t = strings.logs.contextDialog;
/** Lines before and after the entry: the first view, then each "Show more". */
export const AROUND_STEPS = [20, 50, 100, 200] as const;

/**
 * An entry among those logged around it, from every source: what happened elsewhere on the
 * network at that moment. The entry is marked and scrolled into view.
 */
export function ContextDialog(props: {
  record: LogRecord | undefined;
  onClose: () => void;
  source: (record: LogRecord) => SourceLabel;
  pattern: (id: string | null) => LogPattern | undefined;
}) {
  const [step, setStep] = createSignal(0);
  const [answer, setAnswer] = createSignal<{ key: string; records: LogRecord[] }>();
  const [failedKey, setFailedKey] = createSignal<string>();
  const [expanded, setExpanded] = createSignal<number>();
  let list: HTMLDivElement | undefined;
  const around = () => AROUND_STEPS[step()] ?? 20;
  const key = createMemo(() => {
    const record = props.record;
    return record === undefined ? undefined : `${record.id}:${around()}`;
  });
  createEffect(
    () => props.record?.id,
    () => {
      setStep(0);
      setExpanded(undefined);
    },
  );
  createEffect(key, (current) => {
    if (current === undefined) return;
    const [id, n] = current.split(":").map(Number);
    const controller = new AbortController();
    api.logs.context(id ?? 0, n ?? 20, controller.signal).then(
      (context) => {
        setAnswer({ key: current, records: context.records });
        setFailedKey(undefined);
        // The entry in the middle of the view, once it is drawn.
        requestAnimationFrame(() =>
          list?.querySelector("[data-pivot]")?.scrollIntoView({ block: "center" }),
        );
      },
      () => {
        if (!controller.signal.aborted) setFailedKey(current);
      },
    );
    return () => controller.abort();
  });
  const records = () => {
    const value = answer();
    return value && props.record && value.key.startsWith(`${props.record.id}:`)
      ? value.records
      : [];
  };
  const loading = () => key() !== undefined && answer()?.key !== key() && failedKey() !== key();

  return (
    <Dialog
      open={props.record !== undefined}
      onOpenChange={(open) => {
        if (!open) props.onClose();
      }}
      title={t.title}
      description={t.description(around())}
      size="wide"
    >
      <div class="flex flex-col gap-3 px-5 pt-2 pb-5">
        <div
          ref={list}
          class="max-h-[60vh] overflow-auto rounded-control border border-edge"
          aria-busy={loading() ? "true" : "false"}
        >
          <Show
            when={records().length > 0}
            fallback={
              <p role="status" class="px-4 py-10 text-center text-fg-subtle text-sm">
                {failedKey() === key() ? strings.logs.failed : `${t.loading}…`}
              </p>
            }
          >
            <ul aria-label={t.list}>
              <For each={records()} keyed={(record) => record.id}>
                {(record) => {
                  const pivot = () => record().id === props.record?.id;
                  return (
                    <li
                      data-pivot={pivot() ? "" : undefined}
                      aria-current={pivot() ? "true" : undefined}
                    >
                      <LogRow
                        record={record()}
                        source={props.source(record())}
                        pattern={props.pattern(record().pattern)}
                        terms={[]}
                        pivot={pivot()}
                        expanded={expanded() === record().id}
                        onToggle={() =>
                          setExpanded((id) => (id === record().id ? undefined : record().id))
                        }
                        onContext={() => setExpanded(undefined)}
                        onOnlySource={() => undefined}
                        onOnlyProgram={() => undefined}
                        compact
                      />
                    </li>
                  );
                }}
              </For>
            </ul>
          </Show>
        </div>
        <div class="flex flex-wrap items-center justify-between gap-2 text-fg-subtle text-xs">
          <span>{t.hint}</span>
          <Show when={step() < AROUND_STEPS.length - 1}>
            <Button
              size="sm"
              icon={ChevronsUpDown}
              disabled={loading()}
              onClick={() => setStep((n) => Math.min(AROUND_STEPS.length - 1, n + 1))}
            >
              {t.more}
            </Button>
          </Show>
        </div>
      </div>
    </Dialog>
  );
}
