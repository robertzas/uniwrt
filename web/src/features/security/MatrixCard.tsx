import { Ban, Check, CornerDownLeft } from "lucide";
import { createMemo, createSignal, For, Show } from "solid-js";
import type { SiteView } from "../../api/gen/SiteView";
import { errorText } from "../../lib/api";
import { saveSite, sortNetworks } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { SavedRevision } from "../../ui/SavedRevision";
import {
  type Cell,
  cellOf,
  type Grid,
  gridOf,
  gridsDiffer,
  policiesOf,
  seededGrid,
  WAN,
  withCell,
} from "./matrix";

const t = strings.security.matrix;

/**
 * The inter-network matrix (network.md, Firewall policy): each network's row says which
 * networks and whether the internet its clients may reach. Admins toggle cells and save them
 * as one revision; cells that differ from the purpose's default are marked.
 */
export function MatrixCard(props: { view: SiteView; admin: boolean }) {
  const networks = createMemo(() => sortNetworks(props.view.config.networks));
  const saved = createMemo(() => gridOf(networks(), props.view.config.firewall.policies));
  const [draft, setDraft] = createSignal<Grid>();
  const [busy, setBusy] = createSignal(false);
  const [failure, setFailure] = createSignal<string>();
  const [revision, setRevision] = createSignal<number>();
  const grid = () => draft() ?? saved();
  const columns = () => [...networks().map((n) => ({ id: n.id, name: n.name })), internet];
  const nameOf = (id: string) => columns().find((c) => c.id === id)?.name ?? id;

  const edit = () => {
    setDraft(saved());
    setFailure(undefined);
    setRevision(undefined);
  };
  const save = async () => {
    const next = draft();
    if (!next) return;
    if (!gridsDiffer(next, saved())) {
      setDraft(undefined);
      return;
    }
    setBusy(true);
    setFailure(undefined);
    try {
      const view = await saveSite(
        (config) => ({
          ...config,
          firewall: {
            ...config.firewall,
            policies: policiesOf(next, config.networks, config.firewall.policies),
          },
        }),
        t.message,
      );
      setRevision(view.revision.id);
      setDraft(undefined);
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin && !draft()}>
            <Button size="sm" onClick={edit}>
              {strings.security.edit}
            </Button>
          </Show>
        }
      />
      <div class="overflow-x-auto px-4">
        <table class="border-separate border-spacing-1 text-sm" aria-label={t.label}>
          <thead>
            <tr>
              <th scope="col" class="px-2 text-left font-medium text-fg-subtle text-xs">
                {t.from}
              </th>
              <For each={columns()}>
                {(column) => (
                  <th scope="col" class="w-36 px-2 text-left font-medium text-fg-muted text-xs">
                    {column.name}
                  </th>
                )}
              </For>
            </tr>
          </thead>
          <tbody>
            <For each={networks()}>
              {(from) => (
                <tr>
                  <th scope="row" class="whitespace-nowrap px-2 text-left font-medium">
                    {from.name}
                  </th>
                  <For each={columns()}>
                    {(column) => (
                      <td>
                        <Show
                          when={column.id !== from.id}
                          fallback={
                            <span class="flex h-8 items-center px-2 text-fg-subtle">
                              <span aria-hidden="true">—</span>
                              <span class="sr-only">{t.self}</span>
                            </span>
                          }
                        >
                          <CellView
                            cell={cellOf(grid(), from, column.id)}
                            label={(state) => t.cell(from.name, nameOf(column.id), state)}
                            editing={draft() !== undefined}
                            disabled={busy()}
                            onToggle={(allowed) =>
                              setDraft((now) => now && withCell(now, from.id, column.id, allowed))
                            }
                          />
                        </Show>
                      </td>
                    )}
                  </For>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </div>
      <div class="flex flex-col gap-3 px-4 pt-2 pb-4">
        <p class="text-fg-subtle text-xs">{t.legend}</p>
        <Show when={draft()}>
          <div class="flex flex-wrap items-center gap-2">
            <Button
              size="sm"
              variant="ghost"
              disabled={busy()}
              onClick={() => setDraft(seededGrid(networks()))}
            >
              {t.reset}
            </Button>
            <span class="flex-1" />
            <Button variant="ghost" onClick={() => setDraft(undefined)} disabled={busy()}>
              {strings.security.cancel}
            </Button>
            <Button variant="primary" onClick={() => void save()} disabled={busy()}>
              {strings.security.save}
            </Button>
          </div>
        </Show>
        <RequestError text={failure()} />
        <Show when={revision()}>{(id) => <SavedRevision revision={id()} />}</Show>
      </div>
    </Card>
  );
}

const internet = { id: WAN, name: t.internet };

/** One cell: its state as a label (a toggle while editing), marked when changed. */
function CellView(props: {
  cell: Cell;
  label: (state: string) => string;
  editing: boolean;
  disabled: boolean;
  onToggle: (allowed: boolean) => void;
}) {
  const state = () =>
    props.cell.allowed ? t.allow : props.cell.repliesOnly ? t.repliesOnly : t.block;
  const icon = () => (props.cell.allowed ? Check : props.cell.repliesOnly ? CornerDownLeft : Ban);
  const tone = () =>
    props.cell.allowed
      ? "border-ok/40 bg-ok/10 [&_svg]:text-ok"
      : props.cell.repliesOnly
        ? "border-warn/40 bg-warn/10 [&_svg]:text-warn"
        : "border-crit/30 bg-crit/5 [&_svg]:text-crit";
  const name = () => props.label(props.cell.changed ? `${state()}. ${t.changed}` : state());
  const body = () => (
    <>
      <Icon icon={icon()} class="size-3.5 shrink-0" />
      <span class="truncate">{state()}</span>
      <Show when={props.cell.changed}>
        <span class="ml-auto size-1.5 shrink-0 rounded-full bg-accent" title={t.changed} />
      </Show>
    </>
  );
  const base = "flex h-8 w-full items-center gap-1.5 rounded-control border px-2 text-xs";
  return (
    <Show
      when={props.editing}
      fallback={
        <span class={[base, tone()]} title={props.cell.changed ? t.changed : undefined}>
          <span class="sr-only">{name()}</span>
          <span class="contents" aria-hidden="true">
            {body()}
          </span>
        </span>
      }
    >
      <button
        type="button"
        class={[
          base,
          tone(),
          "cursor-pointer hover:border-accent focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-60",
        ]}
        aria-pressed={props.cell.allowed ? "true" : "false"}
        aria-label={name()}
        title={props.cell.changed ? t.changed : undefined}
        disabled={props.disabled}
        onClick={() => props.onToggle(!props.cell.allowed)}
      >
        {body()}
      </button>
    </Show>
  );
}
