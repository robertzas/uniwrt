import { ArrowRight, KeyRound, Minus, PenLine, Plus } from "lucide";
import { createMemo, For, Match, Show, Switch } from "solid-js";
import type { Change } from "../../../api/gen/Change";
import type { ChangeOp } from "../../../api/gen/ChangeOp";
import { strings } from "../../../lib/strings";
import { Icon, type IconNode } from "../../../ui/Icon";
import { entry, type Shown } from "./changes";

const t = strings.settings.revisions;

const OPS: Record<ChangeOp, { icon: IconNode; tone: string }> = {
  added: { icon: Plus, tone: "text-ok" },
  removed: { icon: Minus, tone: "text-crit" },
  changed: { icon: PenLine, tone: "text-info" },
};

/** A path as `networks › iot › vlan`. */
function Path(props: { path: readonly string[]; class?: string }) {
  return (
    <span class={["break-words font-mono", props.class]}>
      <For each={props.path}>
        {(segment, i) => (
          <>
            <Show when={i() > 0}>
              <span class="text-fg-subtle">{" › "}</span>
            </Show>
            {segment}
          </>
        )}
      </For>
    </span>
  );
}

/** A secret's place: a key and "secret", never the value or its reference. */
function Secret() {
  return (
    <span title={t.secretHint} class="inline-flex items-center gap-1 text-fg-muted italic">
      <Icon icon={KeyRound} class="size-3 text-fg-subtle" />
      {t.secret}
    </span>
  );
}

/** A value: text, a secret, or an item's fields one per line. */
function Value(props: { value: Shown; muted?: boolean }) {
  const tone = () => (props.muted ? "text-fg-muted" : "text-fg");
  return (
    <Switch>
      <Match when={props.value.kind === "secret"}>
        <Secret />
      </Match>
      <Match when={props.value.kind === "text" && props.value}>
        {(value) => (
          <span class={["break-words font-mono", tone()]}>
            {(value() as { text: string }).text}
          </span>
        )}
      </Match>
      <Match when={props.value.kind === "fields" && props.value}>
        {(value) => (
          <dl class="grid grid-cols-[minmax(0,max-content)_minmax(0,1fr)] gap-x-3 gap-y-0.5">
            <For each={(value() as Extract<Shown, { kind: "fields" }>).fields}>
              {(field) => (
                <>
                  <dt class="text-fg-subtle">
                    <Path path={field.path} />
                  </dt>
                  <dd>
                    <Value value={field.value} muted={props.muted ?? false} />
                  </dd>
                </>
              )}
            </For>
          </dl>
        )}
      </Match>
    </Switch>
  );
}

/** A revision's changes, one per line in document order: what, where, and the values. */
export function ChangeList(props: {
  changes: readonly Change[];
  nodeName: (id: number) => string | undefined;
}) {
  const entries = createMemo(() => props.changes.map((change) => entry(change, props.nodeName)));
  return (
    <ul class="overflow-hidden rounded-lg border border-edge bg-canvas text-xs">
      <For each={entries()}>
        {(e) => (
          <li class="flex items-start gap-2.5 border-edge/60 border-b px-3 py-2 last:border-b-0">
            <span class={["mt-px shrink-0", OPS[e.op].tone]}>
              <Icon icon={OPS[e.op].icon} class="size-3.5" />
              <span class="sr-only">{t.op[e.op]}</span>
            </span>
            <div class="flex min-w-0 flex-1 flex-col gap-1">
              <Path path={e.path} class="font-medium text-fg" />
              <Switch>
                <Match when={e.secretChanged}>
                  <span class="inline-flex items-center gap-1 text-fg-muted">
                    <Icon icon={KeyRound} class="size-3 text-fg-subtle" />
                    <span title={t.secretHint}>{t.secretChanged}</span>
                  </span>
                </Match>
                <Match when={e.op === "changed" && e.before && e.after}>
                  <div class="flex min-w-0 flex-wrap items-start gap-x-2 gap-y-1">
                    <span class="sr-only">{t.before}: </span>
                    <span class="line-through decoration-fg-subtle/60">
                      <Value value={e.before as Shown} muted />
                    </span>
                    <Icon icon={ArrowRight} class="mt-0.5 size-3 shrink-0 text-fg-subtle" />
                    <span class="sr-only">{t.after}: </span>
                    <Value value={e.after as Shown} />
                  </div>
                </Match>
                <Match when={e.after ?? e.before}>
                  {(value) => <Value value={value()} muted={e.op === "removed"} />}
                </Match>
              </Switch>
            </div>
          </li>
        )}
      </For>
    </ul>
  );
}
