import {
  ArrowRight,
  Cable,
  CircleAlert,
  FilePen,
  Globe,
  Package,
  Power,
  RadioTower,
  Router,
  TriangleAlert,
} from "lucide";
import { For, Show } from "solid-js";
import type { NodePlan } from "../../api/gen/NodePlan";
import type { NodeRole } from "../../api/gen/NodeRole";
import type { PlanAction } from "../../api/gen/PlanAction";
import type { PlanIssue } from "../../api/gen/PlanIssue";
import { formatDuration } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Badge, type Tone } from "../../ui/Badge";
import { Card } from "../../ui/Card";
import { Icon, type IconNode } from "../../ui/Icon";
import { PackageDiff } from "../../ui/PackageDiff";
import { overwrites } from "./plan";
import { RawOverrides } from "./RawOverrides";

const t = strings.changes.node;

const ROLE_ICONS: Record<NodeRole, IconNode> = { gateway: Router, ap: RadioTower, bridge: Cable };
const ACTION_TONES: Record<PlanAction, Tone> = {
  change: "info",
  unchanged: "ok",
  skipped: "neutral",
  refused: "crit",
};

function Issues(props: { title: string; issues: readonly PlanIssue[]; tone: "crit" | "warn" }) {
  return (
    <Show when={props.issues.length > 0}>
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
        <ul class="mt-1 flex flex-col gap-0.5 pl-6">
          <For each={props.issues}>
            {(issue) => (
              <li class="text-fg-muted">
                <span class="font-mono text-fg-subtle text-xs">{issue.path}</span> {issue.message}
              </li>
            )}
          </For>
        </ul>
      </div>
    </Show>
  );
}

/** One node of the plan: what it does, why it can't, and each package's diff. */
export function NodePlanCard(props: { plan: NodePlan }) {
  const p = () => props.plan;
  /** How far it is from the gateway (the gateway's role says it all). */
  const where = () => {
    const hops = p().hops;
    return p().role === "gateway" || hops === null ? undefined : t.hops(hops);
  };
  return (
    <Card aria-label={p().name}>
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1.5 px-4 pt-3.5 pb-3">
        <span class="grid size-8 shrink-0 place-items-center rounded-lg border border-edge bg-raised text-fg-muted">
          <Icon icon={ROLE_ICONS[p().role]} class="size-4" />
        </span>
        <div class="min-w-0 flex-1">
          <a
            href={`/devices/${p().node}?tab=config`}
            aria-label={t.open(p().name)}
            class="font-medium text-fg text-sm hover:underline"
          >
            {p().name}
          </a>
          <p class="text-fg-subtle text-xs">
            {strings.devices.role[p().role]}
            <Show when={where()}>{(text) => <> · {text()}</>}</Show>
            <Show when={p().action === "change"}>
              {" · "}
              {t.timer(formatDuration(p().timeout_s))}
            </Show>
          </p>
        </div>
        <Badge tone={ACTION_TONES[p().action]}>{t.action[p().action]}</Badge>
      </div>
      <Show
        when={
          p().action === "change" || p().action === "refused" || p().reason || p().raw.length > 0
        }
      >
        <div class="flex flex-col gap-3 border-edge/60 border-t px-4 pt-3 pb-4">
          <Show when={p().reason && p().action !== "change"}>
            <p class="text-fg-muted text-sm">{p().reason}</p>
          </Show>
          <Issues title={t.issues} issues={p().issues} tone="crit" />
          <Issues title={t.warnings} issues={p().warnings} tone="warn" />
          <Show when={p().action === "change" && overwrites(p()).length > 0}>
            <p class="flex items-start gap-2 text-fg-muted text-sm">
              <Icon icon={FilePen} class="mt-0.5 size-4 shrink-0 text-warn" />
              {t.drift(overwrites(p()))}
            </p>
          </Show>
          <Show when={p().address}>
            {(address) => (
              <p class="flex items-center gap-2 text-fg-muted text-sm">
                <Icon icon={ArrowRight} class="size-4 shrink-0 text-info" />
                {t.address(address().from, address().to)}
              </p>
            )}
          </Show>
          <Show when={p().renumbering}>
            {(moving) => (
              <p class="flex items-start gap-2 text-fg-muted text-sm" title={t.renumberingHint}>
                <Icon icon={Cable} class="mt-0.5 size-4 shrink-0 text-info" />
                {t.renumbering(moving().from, moving().to)}
              </p>
            )}
          </Show>
          <Show when={p().wan_last}>
            <p class="flex items-center gap-2 text-fg-muted text-sm">
              <Icon icon={Globe} class="size-4 shrink-0 text-info" />
              {t.wanLast}
            </p>
          </Show>
          <Show when={(p().installs ?? []).length > 0}>
            <div class="flex flex-wrap items-center gap-1.5 text-sm" title={t.installsHint}>
              <span class="mr-1 flex items-center gap-1.5 text-fg-muted">
                <Icon icon={Package} class="size-4 text-fg-subtle" />
                {t.installs}
              </span>
              <For each={p().installs}>
                {(name) => (
                  <Badge tone="info">
                    <span class="font-mono">{name}</span>
                  </Badge>
                )}
              </For>
            </div>
          </Show>
          <Show when={p().services.length > 0}>
            <div class="flex flex-wrap items-center gap-1.5 text-sm">
              <span class="mr-1 flex items-center gap-1.5 text-fg-muted">
                <Icon icon={Power} class="size-4 text-fg-subtle" />
                {t.services}
              </span>
              <For each={p().services}>
                {(service) => (
                  <Badge tone={service.enable ? "ok" : "neutral"}>
                    <span class="font-mono">{service.name}</span>
                    <span class="text-fg-muted">{service.enable ? t.on : t.off}</span>
                  </Badge>
                )}
              </For>
            </div>
          </Show>
          <Show when={p().action === "change"}>
            <Show
              when={p().packages.length > 0}
              fallback={<p class="text-fg-muted text-sm">{t.noPackages}</p>}
            >
              <div class="flex flex-col gap-2">
                <p class="text-fg-subtle text-xs">{t.packages(p().packages.length)}</p>
                <For each={p().packages}>
                  {(diff) => (
                    <PackageDiff
                      diff={diff}
                      badge={
                        <Show when={p().drift.includes(diff.package)}>
                          <span title={t.driftHint}>
                            <Badge tone="warn">{t.drifted}</Badge>
                          </span>
                        </Show>
                      }
                    />
                  )}
                </For>
              </div>
            </Show>
          </Show>
          <Show when={p().raw.length > 0}>
            <RawOverrides raw={p().raw} />
          </Show>
        </div>
      </Show>
    </Card>
  );
}
