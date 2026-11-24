import { useQuery } from "@tanstack/solid-query";
import { FileCog, Plus, RotateCw, Search } from "lucide";
import { createMemo, createSignal, For, Match, Show, Switch, useContext } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import type { NodeUci } from "../../../api/gen/NodeUci";
import type { SiteView } from "../../../api/gen/SiteView";
import type { UciOwner } from "../../../api/gen/UciOwner";
import type { UciTreePackage } from "../../../api/gen/UciTreePackage";
import { api, errorText } from "../../../lib/api";
import { formatDateTime } from "../../../lib/format";
import { LiveContext } from "../../../lib/live";
import { SessionContext } from "../../../lib/session";
import { saveSite } from "../../../lib/site";
import { strings } from "../../../lib/strings";
import { Badge, type Tone } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { SavedRevision } from "../../../ui/SavedRevision";
import { entryOf } from "../config/schedules";
import { addSection, dropRaw, type RawEdit, secretPaths, withRaw } from "./edits";
import { RawLine } from "./RawLine";
import { type Explorer, SectionCard } from "./SectionCard";
import { SectionEditor } from "./SectionEditor";
import { matchingSections, searchWords } from "./uci";

const t = strings.device.uci;
const ownerTones: Record<UciOwner, Tone> = { rendered: "info", patched: "warn", node: "neutral" };

/**
 * A device's UCI tab (`?tab=uci`; observability.md, UCI explorer; D-143): every package of its
 * `/etc/config` read live, searchable, secrets masked, with the site's overrides of each section.
 * On a managed device an admin's edit becomes the device's raw sections in the site's next
 * revision, applied through Review.
 */
export function UciTab(props: { node: Node }) {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const head = () => live.config.head?.id;
  const uci = useQuery(() => ({
    queryKey: ["uci", props.node.id, head()],
    queryFn: ({ signal }) => api.nodes.uci(props.node.id, signal),
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
  }));
  const site = useQuery(() => ({
    queryKey: ["site", head()],
    queryFn: ({ signal }) => api.site.get(signal),
  }));
  return (
    <Card aria-label={t.label(props.node.name)}>
      <CardHeader
        title={t.title}
        description={t.description(props.node.name)}
        actions={
          <>
            <Show when={!uci.isError && uci.data}>
              {(read) => (
                <span class="text-fg-subtle text-xs">{t.read(formatDateTime(read().read_at))}</span>
              )}
            </Show>
            <Button
              size="sm"
              icon={RotateCw}
              disabled={uci.isFetching}
              onClick={() => void uci.refetch()}
            >
              {t.refresh}
            </Button>
          </>
        }
      />
      <div class="px-4 pb-4">
        <Switch>
          <Match when={uci.isError}>
            <div class="flex flex-col items-start gap-2">
              <RequestError text={t.failed(errorText(uci.error) ?? "")} />
              <Button size="sm" onClick={() => void uci.refetch()}>
                {t.retry}
              </Button>
            </div>
          </Match>
          <Match when={uci.data}>
            {(read) => (
              <Browser
                node={props.node}
                read={read()}
                site={site.data}
                admin={session.user.role === "admin"}
              />
            )}
          </Match>
          <Match when={true}>
            <p role="status" class="py-8 text-center text-fg-subtle text-sm">
              {t.reading}
            </p>
          </Match>
        </Switch>
      </div>
    </Card>
  );
}

function Browser(props: { node: Node; read: NodeUci; site: SiteView | undefined; admin: boolean }) {
  const [query, setQuery] = createSignal("");
  const [chosen, setChosen] = createSignal<string>();
  const [saved, setSaved] = createSignal<number>();
  const [failure, setFailure] = createSignal<string>();
  const words = createMemo(() => searchWords(query()));
  const shown = createMemo(() =>
    props.read.packages.flatMap((pkg) => {
      const sections = matchingSections(pkg, words());
      return words().length === 0 || sections.length > 0 ? [{ pkg, sections }] : [];
    }),
  );
  const current = () => {
    const all = shown();
    return all.find((entry) => entry.pkg.name === chosen()) ?? all[0];
  };
  const managed = () => props.node.mode === "managed";
  /** The site as the read counted its raw sections; otherwise the read is behind. */
  const fresh = () => props.site !== undefined && props.site.revision.id === props.read.revision;
  const raw = () => (props.site ? entryOf(props.site.config, props.node.id).raw : []);
  const explorer = (): Explorer => ({
    node: props.node.name,
    raw: raw(),
    editable: props.admin && managed() && fresh(),
    save: async (edit: RawEdit | string, message: string) => {
      if (typeof edit === "string") return edit;
      const base = raw();
      try {
        const view = await saveSite(
          (config) => withRaw(config, props.node.id, base, edit.raw),
          message,
          secretPaths(props.node.id, edit),
        );
        setSaved(view.revision.id);
        return undefined;
      } catch (err) {
        return errorText(err);
      }
    },
  });
  const unmatched = () =>
    props.read.unmatched.flatMap((i) => {
      const entry = raw()[i];
      return entry && fresh() ? [{ index: i, raw: entry }] : [];
    });

  return (
    <div class="flex flex-col gap-4">
      <Show when={!managed()}>
        <p class="text-fg-muted text-sm">{t.observed}</p>
      </Show>
      <Show when={managed() && !props.admin}>
        <p class="text-fg-muted text-sm">{t.viewer}</p>
      </Show>
      <Show when={managed() && props.site && !fresh()}>
        <p class="text-sm text-warn" role="status">
          {t.stale}
        </p>
      </Show>
      <Show when={saved()}>
        {(revision) => <SavedRevision revision={revision()} note={t.saved} />}
      </Show>
      <Show when={unmatched().length > 0}>
        <section
          class="flex flex-col gap-2 rounded-control border border-edge px-3 py-2.5"
          aria-label={t.pending.title}
        >
          <h3 class="font-semibold text-sm">{t.pending.title}</h3>
          <p class="text-fg-muted text-xs">{t.pending.description}</p>
          <ul class="flex flex-col gap-1">
            <For each={unmatched()}>
              {(entry) => (
                <li class="flex items-start gap-2">
                  <RawLine raw={entry.raw} />
                  <Show when={explorer().editable}>
                    <Button
                      size="sm"
                      variant="ghost"
                      title={t.dropHint}
                      onClick={async () =>
                        setFailure(
                          await explorer().save(
                            dropRaw(raw(), [entry.index]),
                            t.message.drop(entry.raw.package, props.node.name),
                          ),
                        )
                      }
                    >
                      {t.drop}
                    </Button>
                  </Show>
                </li>
              )}
            </For>
          </ul>
          <RequestError text={failure()} />
        </section>
      </Show>
      <div class="grid gap-4 lg:grid-cols-[15rem_minmax(0,1fr)]">
        <nav class="flex flex-col gap-2" aria-label={t.packages}>
          <label class="relative block">
            <span class="sr-only">{t.search}</span>
            <Icon
              icon={Search}
              class="-translate-y-1/2 pointer-events-none absolute top-1/2 left-2.5 size-3.5 text-fg-subtle"
            />
            <input
              type="search"
              value={query()}
              onInput={(event) => setQuery(event.currentTarget.value)}
              placeholder={t.searchPlaceholder}
              class="h-8 w-full rounded-control border border-edge bg-canvas pr-2 pl-8 text-sm outline-none focus:border-accent focus:ring-2 focus:ring-accent/30"
            />
          </label>
          <Show
            when={shown().length > 0}
            fallback={
              <p class="py-4 text-center text-fg-subtle text-sm">
                {props.read.packages.length === 0 ? t.noPackages : t.noMatch}
              </p>
            }
          >
            <ul class="flex max-h-[32rem] flex-col gap-0.5 overflow-y-auto">
              <For each={shown()}>
                {(entry) => (
                  <li>
                    <button
                      type="button"
                      aria-current={current()?.pkg.name === entry.pkg.name ? "true" : undefined}
                      onClick={() => setChosen(entry.pkg.name)}
                      class="flex w-full items-center gap-2 rounded-control px-2.5 py-1.5 text-left text-sm hover:bg-raised aria-[current]:bg-raised aria-[current]:font-semibold"
                    >
                      <span class="min-w-0 flex-1 truncate font-mono">{entry.pkg.name}</span>
                      <span class="text-fg-subtle text-xs">
                        {words().length > 0 ? entry.sections.length : entry.pkg.sections.length}
                      </span>
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </nav>
        <Show when={current()}>
          {(entry) => (
            <PackageView pkg={entry().pkg} sections={entry().sections} explorer={explorer()} />
          )}
        </Show>
      </div>
    </div>
  );
}

function PackageView(props: { pkg: UciTreePackage; sections: number[]; explorer: Explorer }) {
  const [adding, setAdding] = createSignal(false);
  const editable = () => props.explorer.editable && props.pkg.error === null;
  return (
    <section class="flex min-w-0 flex-col gap-3" aria-label={props.pkg.name}>
      <header class="flex flex-wrap items-center gap-2">
        <Icon icon={FileCog} class="size-4 text-fg-muted" />
        <h3 class="font-mono font-semibold">{props.pkg.name}</h3>
        <Badge tone={ownerTones[props.pkg.owner]}>{t.owner[props.pkg.owner]}</Badge>
        <span class="text-fg-subtle text-xs">{t.sections(props.pkg.sections.length)}</span>
        <Show when={editable() && !adding()}>
          <Button size="sm" icon={Plus} class="ml-auto" onClick={() => setAdding(true)}>
            {t.add}
          </Button>
        </Show>
      </header>
      <p class="text-fg-muted text-xs">{t.ownerHint[props.pkg.owner]}</p>
      <Show when={props.pkg.error}>{(error) => <RequestError text={t.unreadable(error())} />}</Show>
      <Show when={adding()}>
        <div class="rounded-control border border-accent/40 px-3 py-2.5">
          <SectionEditor
            title={t.editor.add(props.pkg.name)}
            taken={props.pkg.sections.flatMap((s) => (s.name === null ? [] : [s.name]))}
            onCancel={() => setAdding(false)}
            onSave={async (delta, added) => {
              if (!added) return undefined;
              const target = `${props.pkg.name}.${added.name ?? `@${added.type}`}`;
              const failed = await props.explorer.save(
                addSection(props.explorer.raw, props.pkg.name, added.type, added.name, delta),
                t.message.add(target, props.explorer.node),
              );
              if (!failed) setAdding(false);
              return failed;
            }}
          />
        </div>
      </Show>
      <Show
        when={props.pkg.sections.length > 0}
        fallback={
          <Show when={props.pkg.error === null}>
            <p class="py-4 text-center text-fg-subtle text-sm">{t.empty}</p>
          </Show>
        }
      >
        <For each={props.sections}>
          {(index) => <SectionCard pkg={props.pkg} index={index} explorer={props.explorer} />}
        </For>
      </Show>
    </section>
  );
}
