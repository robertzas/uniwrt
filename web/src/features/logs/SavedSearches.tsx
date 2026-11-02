import * as Popover from "@kobalte/core/popover";
import { Bookmark, BookmarkPlus, Check, Trash2 } from "lucide";
import { createSignal, For, onSettled, Show } from "solid-js";
import type { SavedSearch } from "../../api/gen/SavedSearch";
import { ApiError, api } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { TextField } from "../../ui/TextField";
import { showToast } from "../../ui/Toast";
import { type Filter, isFiltered, sameFilter, savedFilter } from "./query";

const t = strings.logs.saved;

/**
 * Saved searches, shared by every user: apply one, delete one, or save the current filters under
 * a name. A search keeps the filters, not the time range.
 */
export function SavedSearches(props: { filter: Filter; onApply: (filter: Filter) => void }) {
  const [searches, setSearches] = createSignal<SavedSearch[]>();
  const [open, setOpen] = createSignal(false);
  const [saving, setSaving] = createSignal(false);
  const read = () =>
    api.logs.searches().then(
      (answer) => setSearches(answer.searches),
      () => setSearches((now) => now ?? []),
    );
  onSettled(() => {
    void read();
  });
  const active = () => (searches() ?? []).find((s) => sameFilter(s.filter, props.filter));
  const remove = async (search: SavedSearch) => {
    try {
      await api.logs.remove(search.id);
      showToast({ title: t.deleted(search.name), tone: "ok" });
    } catch (err) {
      showToast({ title: err instanceof ApiError ? err.message : t.deleteFailed, tone: "crit" });
    }
    await read();
  };
  return (
    <>
      <Popover.Root
        open={open()}
        onOpenChange={(next) => {
          setOpen(next);
          if (next) void read();
        }}
        placement="bottom-end"
        gutter={6}
      >
        <Popover.Trigger
          class="inline-flex h-8 items-center gap-2 rounded-control border border-edge px-2.5 text-fg-muted text-sm transition-colors hover:bg-raised hover:text-fg data-expanded:bg-raised focus-visible:outline-2 focus-visible:outline-accent"
          aria-label={active() ? `${t.label}: ${active()?.name}` : t.label}
        >
          <Icon icon={Bookmark} class="size-4 shrink-0 text-fg-subtle" />
          <span class="max-w-[10rem] truncate">{active()?.name ?? t.short}</span>
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content class="z-50 flex max-h-[min(28rem,70vh)] w-80 flex-col rounded-card border border-edge bg-surface shadow-overlay outline-none glass:backdrop-blur-panel data-expanded:animate-pop-in data-closed:animate-pop-out">
            <Popover.Title class="border-edge border-b px-3 py-2 font-medium text-sm">
              {t.label}
            </Popover.Title>
            <div class="min-h-0 flex-1 overflow-auto p-1.5">
              <Show
                when={(searches() ?? []).length > 0}
                fallback={
                  <p class="px-2 py-6 text-center text-fg-subtle text-sm">
                    {searches() === undefined ? `${t.loading}…` : t.none}
                  </p>
                }
              >
                <ul aria-label={t.label}>
                  <For each={searches() ?? []}>
                    {(search) => (
                      <li class="group flex items-center gap-1 rounded-control hover:bg-raised">
                        <button
                          type="button"
                          onClick={() => {
                            props.onApply(search.filter);
                            setOpen(false);
                          }}
                          class="flex min-w-0 flex-1 items-center gap-2 rounded-control px-2 py-1.5 text-left text-sm focus-visible:outline-2 focus-visible:outline-accent"
                        >
                          <Icon
                            icon={Check}
                            class={[
                              "size-3.5 shrink-0 text-accent",
                              { invisible: active()?.id !== search.id },
                            ]}
                          />
                          <span class="truncate">{search.name}</span>
                        </button>
                        <Button
                          size="sm"
                          variant="ghost"
                          icon={Trash2}
                          aria-label={t.delete(search.name)}
                          onClick={() => void remove(search)}
                        />
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            </div>
            <div class="border-edge border-t p-1.5">
              <button
                type="button"
                disabled={!isFiltered(props.filter)}
                title={isFiltered(props.filter) ? undefined : t.nothingToSave}
                onClick={() => {
                  setOpen(false);
                  setSaving(true);
                }}
                class="flex h-8 w-full items-center gap-2 rounded-control px-2 font-medium text-accent text-sm hover:bg-raised disabled:cursor-not-allowed disabled:text-fg-subtle disabled:hover:bg-transparent"
              >
                <Icon icon={BookmarkPlus} class="size-4" />
                {t.save}
              </button>
            </div>
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
      <SaveDialog
        open={saving()}
        filter={props.filter}
        onOpenChange={setSaving}
        onSaved={() => void read()}
      />
    </>
  );
}

function SaveDialog(props: {
  open: boolean;
  filter: Filter;
  onOpenChange: (open: boolean) => void;
  onSaved: () => void;
}) {
  const [name, setName] = createSignal("");
  const [error, setError] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    const text = name().trim();
    if (text === "") {
      setError(t.nameRequired);
      return;
    }
    setBusy(true);
    try {
      await api.logs.save(text, savedFilter(props.filter));
      showToast({ title: t.savedToast(text), tone: "ok" });
      setName("");
      setError(undefined);
      props.onSaved();
      props.onOpenChange(false);
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t.failed);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open={props.open}
      onOpenChange={(open) => {
        if (!open) setError(undefined);
        props.onOpenChange(open);
      }}
      title={t.title}
      description={t.description}
    >
      <form class="flex flex-col gap-4 px-5 pt-2 pb-5" onSubmit={(event) => void submit(event)}>
        <TextField
          label={t.name}
          value={name()}
          onInput={(value) => {
            setName(value);
            setError(undefined);
          }}
          error={error()}
          autofocus
          required
        />
        <div class="flex justify-end gap-2">
          <Button variant="ghost" onClick={() => props.onOpenChange(false)}>
            {t.cancel}
          </Button>
          <Button type="submit" variant="primary" disabled={busy()}>
            {t.submit}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
