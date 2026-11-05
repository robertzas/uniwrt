import type { JSX } from "@solidjs/web";
import { Cable, ChevronRight, CircleAlert, RadioTower, Router, TriangleAlert } from "lucide";
import { createMemo, createSignal, For, Show } from "solid-js";
import type { NodeImportState } from "../../../api/gen/NodeImportState";
import type { NodeImportView } from "../../../api/gen/NodeImportView";
import type { NodeRole } from "../../../api/gen/NodeRole";
import type { SectionFate } from "../../../api/gen/SectionFate";
import { strings } from "../../../lib/strings";
import { Badge, type Tone } from "../../../ui/Badge";
import { Card } from "../../../ui/Card";
import { Icon, type IconNode } from "../../../ui/Icon";
import { PackageDiff } from "../../../ui/PackageDiff";
import { byPackage, fateCounts, type Repeated } from "./preview";

const t = strings.settings.import.node;

const ROLE_ICONS: Record<NodeRole, IconNode> = { gateway: Router, ap: RadioTower, bridge: Cable };
const STATE_TONES: Record<NodeImportState, Tone> = {
  imported: "ok",
  failed: "crit",
  unread: "warn",
};
const FATE_TONES: Record<SectionFate, string> = {
  mapped: "text-ok",
  raw: "text-info",
  dropped: "text-fg-subtle",
};

/** A tinted block of messages: why it failed (crit) or what to know (warn). */
function Notes(props: { title: string; tone: "crit" | "warn"; children: JSX.Element }) {
  return (
    <div
      role={props.tone === "crit" ? "alert" : undefined}
      class={[
        "rounded-control border px-3 py-2 text-sm",
        props.tone === "crit" ? "border-crit/40 bg-crit/10" : "border-warn/40 bg-warn/10",
      ]}
    >
      <p class="flex items-center gap-2 font-medium text-fg">
        <Icon
          icon={props.tone === "crit" ? CircleAlert : TriangleAlert}
          class={["size-4 shrink-0", props.tone === "crit" ? "text-crit" : "text-warn"]}
        />
        {props.title}
      </p>
      <ul class="mt-1 flex flex-col gap-0.5 pl-6 text-fg-muted">{props.children}</ul>
    </div>
  );
}

/** The share of its options the model holds, as a bar and a number. */
function Confidence(props: { value: number }) {
  return (
    <div class="flex items-center gap-2.5" title={t.confidenceHint}>
      <span class="h-1.5 w-28 overflow-hidden rounded-full bg-raised" aria-hidden="true">
        <span
          class={[
            "block h-full rounded-full",
            props.value >= 85 ? "bg-ok" : props.value >= 60 ? "bg-info" : "bg-warn",
          ]}
          style={{ width: `${Math.max(0, Math.min(100, props.value))}%` }}
        />
      </span>
      <span class="text-fg-muted text-xs tabular-nums">{t.confidence(props.value)}</span>
    </div>
  );
}

/** Every section of the node by package and its fate, closed until opened. */
function Sections(props: { node: NodeImportView }) {
  const [open, setOpen] = createSignal(false);
  const c = createMemo(() => fateCounts(props.node.sections));
  const groups = createMemo(() => byPackage(props.node.sections));
  return (
    <div class="overflow-hidden rounded-control border border-edge">
      <button
        type="button"
        aria-expanded={open() ? "true" : "false"}
        aria-label={t.toggle(props.node.name)}
        onClick={() => setOpen((now) => !now)}
        class="flex w-full items-center gap-2 bg-raised/50 px-3 py-1.5 text-left hover:bg-raised focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
      >
        <Icon
          icon={ChevronRight}
          class={["size-3.5 shrink-0 text-fg-subtle transition-transform", { "rotate-90": open() }]}
        />
        <span class="font-medium text-fg text-xs">{t.sections}</span>
        <span class="ml-auto text-fg-subtle text-xs tabular-nums">
          {t.sectionCounts(c().mapped, c().raw, c().dropped)}
        </span>
      </button>
      <Show when={open()}>
        <div class="divide-y divide-edge/50 border-edge/60 border-t font-mono text-xs leading-5">
          <For each={groups()}>
            {(group) => (
              <section aria-label={`/etc/config/${group.package}`} class="py-1">
                <h3 class="px-3 py-0.5 font-medium text-fg">/etc/config/{group.package}</h3>
                <ul>
                  <For each={group.sections}>
                    {(section) => (
                      <li class="flex min-w-0 flex-wrap items-baseline gap-x-2 py-0.5 pr-3 pl-6">
                        <span class="min-w-0">
                          <span class="text-fg-subtle">config</span>{" "}
                          <span class="text-fg">{section.type}</span>{" "}
                          <span class="text-fg">
                            {section.name === null
                              ? t.anonymous(section.type)
                              : `'${section.name}'`}
                          </span>
                          <Show when={section.renamed}>
                            {(to) => <span class="text-fg-subtle"> {t.renamed(to())}</span>}
                          </Show>
                        </span>
                        <span
                          class={["font-sans text-[0.6875rem]", FATE_TONES[section.fate]]}
                          title={t.fateHint[section.fate]}
                        >
                          {t.fate[section.fate]}
                        </span>
                        <Show when={section.kept.length > 0}>
                          <span class="font-sans text-[0.6875rem] text-fg-muted">
                            · {t.kept(section.kept.join(", "))}
                          </span>
                        </Show>
                      </li>
                    )}
                  </For>
                </ul>
              </section>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}

/**
 * One node of an import: whether it was read, how much of it the model holds, its sections'
 * fates, what the first apply would still change (the residual), and its warnings, each
 * message once (`site` holds the document's warnings at `nodes/<id>/…`).
 */
export function NodeImportCard(props: { node: NodeImportView; site: readonly Repeated[] }) {
  const n = () => props.node;
  return (
    <Card aria-label={n().name}>
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1.5 px-4 pt-3.5 pb-3">
        <span class="grid size-8 shrink-0 place-items-center rounded-lg border border-edge bg-raised text-fg-muted">
          <Icon icon={ROLE_ICONS[n().role]} class="size-4" />
        </span>
        <div class="min-w-0 flex-1">
          <a
            href={`/devices/${n().node}?tab=config`}
            aria-label={t.open(n().name)}
            class="font-medium text-fg text-sm hover:underline"
          >
            {n().name}
          </a>
          <p class="text-fg-subtle text-xs">
            {strings.devices.role[n().role]} · {t.mode[n().mode]}
          </p>
        </div>
        <Show when={n().state === "imported"}>
          <Confidence value={n().confidence} />
        </Show>
        <Badge tone={STATE_TONES[n().state]}>{t.state[n().state]}</Badge>
      </div>
      <div class="flex flex-col gap-3 border-edge/60 border-t px-4 pt-3 pb-4">
        <Show when={n().state === "unread"}>
          <p class="text-fg-muted text-sm">
            {n().reason} <span class="text-fg-subtle">{t.unreadHint}</span>
          </p>
        </Show>
        <Show when={n().issues.length > 0}>
          <Notes title={t.issues} tone="crit">
            <For each={n().issues}>
              {(issue) => (
                <li>
                  <span class="font-mono text-fg-subtle text-xs">{issue.path}</span> {issue.message}
                </li>
              )}
            </For>
          </Notes>
        </Show>
        <Show when={n().warnings.length > 0}>
          <Notes title={t.warnings} tone="warn">
            <For each={n().warnings}>
              {(issue) => (
                <li>
                  <span class="font-mono text-fg-subtle text-xs">{issue.path}</span> {issue.message}
                </li>
              )}
            </For>
          </Notes>
        </Show>
        <Show when={props.site.length > 0}>
          <Notes title={t.siteWarnings} tone="warn">
            <For each={props.site}>
              {(note) => (
                <li>
                  {note.message}
                  <Show when={note.count > 1}>
                    <span class="ml-1.5 text-fg-subtle text-xs tabular-nums">
                      {t.times(note.count)}
                    </span>
                  </Show>
                </li>
              )}
            </For>
          </Notes>
        </Show>
        <Show when={n().sections.length > 0}>
          <Sections node={n()} />
        </Show>
        <Show when={n().residual.length > 0}>
          <div class="flex flex-col gap-2">
            <div>
              <p class="font-medium text-fg text-xs">{t.residual}</p>
              <p class="text-fg-subtle text-xs">{t.residualHint}</p>
            </div>
            <For each={n().residual}>{(diff) => <PackageDiff diff={diff} open={false} />}</For>
          </div>
        </Show>
      </div>
    </Card>
  );
}
