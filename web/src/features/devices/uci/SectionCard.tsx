import { Lock, Pencil, Trash2 } from "lucide";
import { createSignal, For, Show } from "solid-js";
import type { RawSection } from "../../../api/gen/RawSection";
import type { UciTreePackage } from "../../../api/gen/UciTreePackage";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { deleteSection, dropRaw, editSection, type RawEdit } from "./edits";
import { RawLine } from "./RawLine";
import { SectionEditor } from "./SectionEditor";
import { sectionLabel } from "./uci";

const t = strings.device.uci;

/** What a package's view needs: the site's raw sections as read, and how to save an edit. */
export interface Explorer {
  node: string;
  raw: readonly RawSection[];
  /** Admin, managed and the site as read: an edit can be saved. */
  editable: boolean;
  /** Saves the edit as the site's next revision; what went wrong, if anything. */
  save: (edit: RawEdit | string, message: string) => Promise<string | undefined>;
}

/** One section as the device holds it: its options, its overrides, and editing it. */
export function SectionCard(props: { pkg: UciTreePackage; index: number; explorer: Explorer }) {
  const section = () => props.pkg.sections[props.index];
  const label = () => sectionLabel(props.pkg, props.index);
  const target = () => `${props.pkg.name}.${label()}`;
  const [editing, setEditing] = createSignal(false);
  const [confirming, setConfirming] = createSignal(false);
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const overrides = () =>
    (section()?.overrides ?? []).flatMap((i) => {
      const raw = props.explorer.raw[i];
      return raw ? [{ index: i, raw }] : [];
    });

  const run = async (edit: RawEdit | string, message: string) => {
    setBusy(true);
    setFailure(await props.explorer.save(edit, message));
    setBusy(false);
  };

  return (
    <Show when={section()}>
      {(s) => (
        <article class="rounded-control border border-edge bg-surface" aria-label={target()}>
          <header class="flex flex-wrap items-center gap-2 border-edge border-b px-3 py-2">
            <h4 class="min-w-0 truncate font-mono font-semibold text-sm">{label()}</h4>
            <Badge>{s().type}</Badge>
            <Show when={overrides().length > 0}>
              <Badge tone="info">{t.overrides(overrides().length)}</Badge>
            </Show>
            <Show when={props.explorer.editable && !editing()}>
              <div class="ml-auto flex gap-1">
                <Button
                  size="sm"
                  variant="ghost"
                  icon={Pencil}
                  onClick={() => {
                    setEditing(true);
                    setConfirming(false);
                    setFailure(undefined);
                  }}
                >
                  {t.edit}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  icon={Trash2}
                  onClick={() => setConfirming(true)}
                  disabled={busy()}
                >
                  {t.remove}
                </Button>
              </div>
            </Show>
          </header>
          <div class="flex flex-col gap-3 px-3 py-2.5">
            <Show when={confirming()}>
              <div
                class="flex flex-wrap items-center gap-2 rounded-control border border-warn/40 bg-warn/10 px-3 py-2 text-sm"
                role="alertdialog"
                aria-label={t.remove}
              >
                <p class="min-w-0 flex-1">{t.removeConfirm(target())}</p>
                <Button size="sm" variant="ghost" onClick={() => setConfirming(false)}>
                  {t.cancel}
                </Button>
                <Button
                  size="sm"
                  variant="primary"
                  disabled={busy()}
                  onClick={() => {
                    setConfirming(false);
                    void run(
                      deleteSection(props.explorer.raw, props.pkg, props.index),
                      t.message.remove(target(), props.explorer.node),
                    );
                  }}
                >
                  {t.removeYes}
                </Button>
              </div>
            </Show>
            <Show
              when={editing()}
              fallback={
                <Show
                  when={s().options.length > 0}
                  fallback={<p class="text-fg-subtle text-xs">{t.noOptions}</p>}
                >
                  <dl class="grid grid-cols-[minmax(0,12rem)_minmax(0,1fr)] gap-x-4 gap-y-1 text-xs">
                    <For each={s().options}>
                      {(option) => (
                        <>
                          <dt class="truncate font-mono text-fg-muted" title={option.name}>
                            {option.name}
                          </dt>
                          <dd class="min-w-0 break-all font-mono">
                            <Show
                              when={Array.isArray(option.value)}
                              fallback={
                                <Value text={option.value as string} secret={option.secret} />
                              }
                            >
                              <ul class="flex flex-col">
                                <For each={option.value as string[]}>
                                  {(item) => (
                                    <li>
                                      <Value text={item} secret={option.secret} />
                                    </li>
                                  )}
                                </For>
                              </ul>
                            </Show>
                          </dd>
                        </>
                      )}
                    </For>
                  </dl>
                </Show>
              }
            >
              <SectionEditor
                title={t.editor.edit(target())}
                section={s()}
                onCancel={() => setEditing(false)}
                onSave={async (delta) => {
                  const failed = await props.explorer.save(
                    editSection(props.explorer.raw, props.pkg, props.index, delta),
                    t.message.edit(target(), props.explorer.node),
                  );
                  if (!failed) setEditing(false);
                  return failed;
                }}
              />
            </Show>
            <RequestError text={failure()} />
            <Show when={overrides().length > 0}>
              <section class="flex flex-col gap-1.5" aria-label={t.overridesTitle}>
                <h5 class="font-medium text-fg-muted text-xs">{t.overridesTitle}</h5>
                <ul class="flex flex-col gap-1">
                  <For each={overrides()}>
                    {(entry) => (
                      <li class="flex items-start gap-2">
                        <RawLine raw={entry.raw} />
                        <Show when={props.explorer.editable}>
                          <Button
                            size="sm"
                            variant="ghost"
                            title={t.dropHint}
                            disabled={busy()}
                            onClick={() =>
                              void run(
                                dropRaw(props.explorer.raw, [entry.index]),
                                t.message.drop(target(), props.explorer.node),
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
              </section>
            </Show>
          </div>
        </article>
      )}
    </Show>
  );
}

/** An option's value, or a mask with a lock for a secret. */
function Value(props: { text: string; secret: boolean }) {
  return (
    <Show when={props.secret} fallback={<span>{props.text}</span>}>
      <span class="inline-flex items-center gap-1 text-fg-subtle" title={t.secret}>
        <Icon icon={Lock} class="size-3" />
        {props.text}
      </span>
    </Show>
  );
}
