import { Package } from "lucide";
import { createSignal, For, Show } from "solid-js";
import type { InstalledPackage } from "../../../api/gen/InstalledPackage";
import type { Job } from "../../../api/gen/Job";
import type { Node } from "../../../api/gen/Node";
import type { PackageCatalog } from "../../../api/gen/PackageCatalog";
import type { PackageChanges } from "../../../api/gen/PackageChanges";
import { api, errorText } from "../../../lib/api";
import { formatBytes } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Dialog } from "../../../ui/Dialog";
import { JobErrorNote, JobSuccessNote, RequestError } from "../../../ui/JobOutcome";
import { JobSteps } from "../../../ui/JobSteps";
import { changeCount, changeRows, installBytes } from "./software";

const k = strings.device.software.confirm;

/**
 * The confirmation of a node's package changes, listing every one (network.md, Package manager;
 * D-139), then the `packages` job it starts, followed live: its Check step says what the dry run
 * adds and removes.
 */
export function ChangesDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  node: Node;
  changes: PackageChanges;
  catalog: PackageCatalog | undefined;
  installed: readonly InstalledPackage[];
  /** The job started here, once it is. */
  job: Job | undefined;
  onStarted: (job: Job) => void;
}) {
  const [sending, setSending] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const size = () => installBytes(props.changes, props.catalog);

  const start = async () => {
    setSending(true);
    setError(undefined);
    try {
      props.onStarted(await api.nodes.changePackages(props.node.id, props.changes));
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSending(false);
    }
  };

  return (
    <Dialog
      open={props.open}
      onOpenChange={(open) => {
        if (!open) setError(undefined);
        props.onOpenChange(open);
      }}
      title={k.title(props.node.name)}
      size="lg"
    >
      <div class="flex flex-col gap-4 px-5 pt-2 pb-5">
        <Show
          when={props.job}
          fallback={
            <>
              <For each={changeRows(props.changes, props.catalog, props.installed)}>
                {(section) => (
                  <section aria-label={k.kinds[section.kind]}>
                    <h3 class="font-medium text-sm">
                      {k.kinds[section.kind]} ({section.rows.length})
                    </h3>
                    <ul class="mt-1.5 max-h-48 divide-y divide-edge overflow-auto rounded-control border border-edge">
                      <For each={section.rows}>
                        {(row) => (
                          <li class="flex items-center justify-between gap-3 px-3 py-1.5 text-sm">
                            <span class="font-mono">{row.name}</span>
                            <span class="text-fg-subtle text-xs">{row.detail}</span>
                          </li>
                        )}
                      </For>
                    </ul>
                  </section>
                )}
              </For>
              <div class="flex flex-col gap-1.5 rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5 text-sm">
                <Show when={size()}>{(bytes) => <p>{k.space(formatBytes(bytes()))}</p>}</Show>
                <p>{k.check}</p>
                <p class="text-fg-muted">{k.after}</p>
              </div>
              <RequestError text={error()} />
              <div class="flex justify-end gap-2">
                <Button variant="ghost" onClick={() => props.onOpenChange(false)}>
                  {k.cancel}
                </Button>
                <Button
                  variant="primary"
                  icon={Package}
                  disabled={sending() || changeCount(props.changes) === 0}
                  onClick={() => void start()}
                >
                  {sending() ? k.starting : k.start(changeCount(props.changes))}
                </Button>
              </div>
            </>
          }
        >
          {(job) => (
            <>
              <JobSteps job={job()} />
              <Show when={job().state === "succeeded"}>
                <JobSuccessNote text={k.done} />
              </Show>
              <Show when={job().error}>{(failure) => <JobErrorNote error={failure()} />}</Show>
              <div class="flex justify-end">
                <Button onClick={() => props.onOpenChange(false)}>{k.close}</Button>
              </div>
            </>
          )}
        </Show>
      </div>
    </Dialog>
  );
}
