import { Pencil, Plus, TriangleAlert } from "lucide";
import { createSignal, For, Show } from "solid-js";
import type { Ddns } from "../../../api/gen/Ddns";
import type { ServiceFacts } from "../../../api/gen/ServiceFacts";
import type { SiteView } from "../../../api/gen/SiteView";
import { strings } from "../../../lib/strings";
import { StatusDot } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { DdnsEditor } from "./DdnsEditor";
import { ddnsStatus, ddnsWarnings } from "./ddns";

const t = strings.networks.ddns;
/** The most DDNS names (`MAX_DDNS` in the model). */
const MAX_DDNS = 16;

/** What the editor is open on: an entry, or a new one, with the document as it opened. */
type Editing = { ddns: Ddns | undefined; view: SiteView };

/**
 * Dynamic DNS (network.md, Other services; D-132): each name with its provider and record, and
 * what the gateway reports (registered address, last update, whether its updater runs). Admins
 * add and edit names in a dialog.
 */
export function DdnsCard(props: {
  view: SiteView;
  admin: boolean;
  services: ServiceFacts | undefined;
}) {
  const [editing, setEditing] = createSignal<Editing>();
  const entries = () => props.view.config.services.ddns;

  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin && entries().length < MAX_DDNS}>
            <Button
              size="sm"
              icon={Plus}
              onClick={() => setEditing({ ddns: undefined, view: props.view })}
            >
              {t.add}
            </Button>
          </Show>
        }
      />
      <Show
        when={entries().length > 0}
        fallback={<p class="px-4 pb-4 text-fg-muted text-sm">{t.none}</p>}
      >
        <ul class="divide-y divide-edge border-edge border-t">
          <For each={entries()}>
            {(ddns, i) => (
              <Row
                ddns={ddns}
                status={ddnsStatus(ddns, props.services?.ddns)}
                warnings={ddnsWarnings(props.view.issues, i())}
                admin={props.admin}
                onEdit={() => setEditing({ ddns, view: props.view })}
              />
            )}
          </For>
        </ul>
      </Show>
      <Show when={editing()}>
        {(open) => (
          <DdnsEditor view={open().view} ddns={open().ddns} onClose={() => setEditing(undefined)} />
        )}
      </Show>
    </Card>
  );
}

function Row(props: {
  ddns: Ddns;
  status: ReturnType<typeof ddnsStatus>;
  warnings: string[];
  admin: boolean;
  onEdit: () => void;
}) {
  const details = () =>
    [
      t.providers[props.ddns.provider.kind],
      t.record[String(props.ddns.ipv6)],
      t.sources[props.ddns.source],
    ].join(" · ");
  return (
    <li class="flex flex-col gap-1 px-4 py-3" aria-label={props.ddns.hostname}>
      <div class="flex flex-wrap items-center gap-x-4 gap-y-1">
        <div class="min-w-0 flex-1">
          <p class="truncate font-mono text-sm">{props.ddns.hostname}</p>
          <p class="text-fg-muted text-xs">{details()}</p>
        </div>
        <p class="flex items-center gap-2 text-xs">
          <StatusDot tone={props.status.tone} />
          <span>{props.status.text}</span>
          <Show when={props.status.updated}>
            {(updated) => <span class="text-fg-subtle">{updated()}</span>}
          </Show>
        </p>
        <Show when={props.admin}>
          <Button
            size="sm"
            variant="ghost"
            icon={Pencil}
            aria-label={t.edit(props.ddns.hostname)}
            onClick={() => props.onEdit()}
          />
        </Show>
      </div>
      <For each={props.warnings}>
        {(warning) => (
          <p class="flex items-center gap-2 text-warn text-xs">
            <Icon icon={TriangleAlert} class="size-3.5 shrink-0" />
            {warning}
          </p>
        )}
      </For>
    </li>
  );
}
