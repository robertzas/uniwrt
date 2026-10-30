import { Fingerprint, LoaderCircle, Radar } from "lucide";
import {
  createMemo,
  createSignal,
  For,
  Match,
  onSettled,
  Show,
  Switch,
  useContext,
} from "solid-js";
import type { Discovered } from "../../../api/gen/Discovered";
import type { DiscoveryReport } from "../../../api/gen/DiscoveryReport";
import { api } from "../../../lib/api";
import { adoptionOf, type JobsState } from "../../../lib/jobs";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Badge, StatusDot } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Checkbox } from "../../../ui/Checkbox";
import { Icon } from "../../../ui/Icon";
import { discoveredName } from "./BatchDialog";

const d = strings.discovered;

/** How often discovery is asked again: soon while probes run, else now and then. */
const SCANNING_MS = 2_000;
const IDLE_MS = 15_000;

export interface DiscoveredCardProps {
  jobs: JobsState;
  /** Opens the wizard for one device. */
  onAdopt: (host: string) => void;
  /** Opens batch adoption for the selected devices. */
  onAdoptMany: (nodes: Discovered[]) => void;
}

/** A device can join a batch once it has an address and a host key, and is not being adopted. */
function selectable(node: Discovered, jobs: JobsState): node is Discovered & { ip: string } {
  return node.ip !== null && node.host_key !== null && !adoptionOf(jobs, node.ip);
}

/** `SHA256:abcdefgh…wxyz`, for a list; dialogs show the whole fingerprint. */
function shortFingerprint(fingerprint: string): string {
  const [algorithm, hash = ""] = fingerprint.split(":", 2);
  return hash.length > 14 ? `${algorithm}:${hash.slice(0, 8)}…${hash.slice(-4)}` : fingerprint;
}

/**
 * `GET v1/discovered` now and again while the calling component lives, skipped while the tab is
 * hidden. A plain signal, not a query: a request takes up to 3 s and repeats every 2 s while
 * probes run, and pending async data would hold every navigation away from the page.
 */
function pollDiscovered() {
  const [report, setReport] = createSignal<DiscoveryReport>();
  onSettled(() => {
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      let next = IDLE_MS;
      if (!document.hidden) {
        try {
          const fresh = await api.discovered(controller.signal);
          setReport(fresh);
          if (fresh.scanning) next = SCANNING_MS;
        } catch {
          // Keep the last report; the next poll tries again.
        }
      }
      if (!controller.signal.aborted) timer = setTimeout(poll, next);
    };
    void poll();
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  });
  return report;
}

/**
 * The unadopted OpenWrt devices the adopted nodes see, polled while the page is open; hidden while
 * there are none. Each can be adopted alone, or several at once.
 */
export function DiscoveredCard(props: DiscoveredCardProps) {
  const live = useContext(LiveContext);
  const report = pollDiscovered();
  const nodes = () => report()?.nodes ?? [];
  const [selected, setSelected] = createSignal<readonly string[]>([]);
  const chosen = createMemo(() =>
    nodes().filter((node) => selected().includes(node.mac) && selectable(node, props.jobs)),
  );
  const toggle = (mac: string, on: boolean) =>
    setSelected((macs) => (on ? [...macs, mac] : macs.filter((m) => m !== mac)));
  const nameOf = (id: number) => live.fleet.nodes.find((node) => node.id === id)?.name;

  return (
    <Show when={nodes().length > 0}>
      <Card class="mx-auto mt-4 max-w-7xl overflow-hidden" aria-label={d.title}>
        <CardHeader
          title={d.title}
          description={d.description}
          actions={
            <>
              <Show when={report()?.scanning}>
                <span role="status" class="flex items-center gap-1.5">
                  <StatusDot tone="info" pulse />
                  {d.scanning}
                </span>
              </Show>
              <Show when={chosen().length > 0}>
                <Button
                  size="sm"
                  variant="primary"
                  onClick={() => {
                    props.onAdoptMany(chosen());
                    setSelected([]);
                  }}
                >
                  {d.adoptSelected(chosen().length)}
                </Button>
              </Show>
            </>
          }
        />
        <ul class="divide-y divide-edge border-edge border-t">
          <For each={nodes()} keyed={(node) => node.mac}>
            {(node) => (
              <Row
                node={node()}
                jobs={props.jobs}
                seenBy={nameOf(node().seen_by)}
                selected={selected().includes(node().mac)}
                onSelect={(on) => toggle(node().mac, on)}
                onAdopt={props.onAdopt}
              />
            )}
          </For>
        </ul>
      </Card>
    </Show>
  );
}

function Row(props: {
  node: Discovered;
  jobs: JobsState;
  seenBy: string | undefined;
  selected: boolean;
  onSelect: (on: boolean) => void;
  onAdopt: (host: string) => void;
}) {
  const name = () => discoveredName(props.node);
  const adopting = () => (props.node.ip ? adoptionOf(props.jobs, props.node.ip) : undefined);
  const sources = () => props.node.sources.map((source) => d.source[source]).join(", ");
  return (
    <li class="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-2.5">
      <Checkbox
        label={d.select(name())}
        hideLabel
        checked={props.selected && selectable(props.node, props.jobs)}
        onChange={props.onSelect}
        disabled={!selectable(props.node, props.jobs)}
      />
      <div class="flex min-w-48 flex-1 items-center gap-2.5">
        <Icon icon={Radar} class="size-4 shrink-0 text-fg-subtle" />
        <div class="min-w-0">
          <p class="truncate font-medium text-sm">{name()}</p>
          <p class="truncate text-fg-subtle text-xs">
            <span class="font-mono">{props.node.mac}</span>
            {" · "}
            {props.seenBy ? `${d.seenBy(props.seenBy)} (${sources()})` : sources()}
          </p>
        </div>
      </div>
      <div class="w-32 font-mono text-xs">
        <Show
          when={props.node.ip}
          fallback={
            <span class="font-sans text-fg-subtle" title={d.noAddressHint}>
              {d.noAddress}
            </span>
          }
        >
          {(ip) => ip()}
        </Show>
      </div>
      <div class="flex w-56 min-w-0 items-center gap-1.5 text-xs">
        <Switch>
          <Match when={props.node.host_key}>
            {(key) => (
              <>
                <Icon icon={Fingerprint} class="size-3.5 shrink-0 text-fg-subtle" />
                <span class="truncate font-mono" title={key().fingerprint}>
                  {shortFingerprint(key().fingerprint)}
                </span>
              </>
            )}
          </Match>
          <Match when={props.node.probe === "probing"}>
            <span class="text-fg-subtle">{d.probing}…</span>
          </Match>
          <Match when={props.node.probe === "unreachable"}>
            <span class="text-fg-subtle">{d.unreachable}</span>
          </Match>
          <Match when={props.node.ip}>
            <span class="text-fg-subtle">{d.noKey}</span>
          </Match>
        </Switch>
      </div>
      <div class="ml-auto flex w-28 justify-end">
        <Show
          when={!adopting()}
          fallback={
            <Badge icon={LoaderCircle} tone="info">
              {d.adopting}
            </Badge>
          }
        >
          <Show when={props.node.ip}>
            {(ip) => (
              <Button size="sm" onClick={() => props.onAdopt(ip())}>
                {d.adopt}
              </Button>
            )}
          </Show>
        </Show>
      </div>
    </li>
  );
}
