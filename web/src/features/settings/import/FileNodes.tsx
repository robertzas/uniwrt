import { ArrowRight } from "lucide";
import { For, Show } from "solid-js";
import type { NodeMatch } from "../../../api/gen/NodeMatch";
import { strings } from "../../../lib/strings";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";

const t = strings.settings.fileImport;

/**
 * The file's devices and the adopted device each maps onto by MAC (one without a match leaves
 * its settings out), then the adopted devices the file doesn't name (they keep theirs).
 */
export function FileNodes(props: {
  nodes: readonly NodeMatch[];
  kept: readonly number[];
  nodeName: (id: number) => string | undefined;
}) {
  const name = (id: number) => props.nodeName(id) ?? `#${id}`;
  return (
    <Card aria-label={t.devices}>
      <CardHeader title={t.devices} description={t.devicesHint} />
      <Show
        when={props.nodes.length + props.kept.length > 0}
        fallback={<p class="px-4 pb-4 text-fg-muted text-sm">{t.noDevices}</p>}
      >
        <ul class="border-edge/60 border-t text-sm">
          <li
            aria-hidden="true"
            class="grid grid-cols-[1fr_auto_1fr] gap-3 px-4 py-1.5 text-fg-subtle text-xs"
          >
            <span>{t.inFile}</span>
            <span class="w-4" />
            <span>{t.here}</span>
          </li>
          <For each={props.nodes}>
            {(match) => (
              <li class="grid grid-cols-[1fr_auto_1fr] items-center gap-3 border-edge/60 border-t px-4 py-2">
                <div class="min-w-0">
                  <p class="truncate font-medium">{match.name}</p>
                  <p class="font-mono text-fg-subtle text-xs">{match.mac}</p>
                </div>
                <Icon icon={ArrowRight} class="size-4 text-fg-subtle" />
                <Show
                  when={match.node}
                  fallback={<p class="text-fg-muted text-xs">{t.unmatched}</p>}
                >
                  {(id) => <p class="truncate font-medium">{name(id())}</p>}
                </Show>
              </li>
            )}
          </For>
          <For each={props.kept}>
            {(id) => (
              <li class="grid grid-cols-[1fr_auto_1fr] items-center gap-3 border-edge/60 border-t px-4 py-2">
                <p class="text-fg-muted text-xs">{t.keeps}</p>
                <span class="w-4" />
                <p class="truncate font-medium">{name(id)}</p>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </Card>
  );
}
