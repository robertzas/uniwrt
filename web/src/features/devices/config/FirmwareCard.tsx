import { useNavigate } from "@solidjs/router";
import { ExternalLink, PackageCheck, PackagePlus } from "lucide";
import { createMemo, For, Show } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import { createCatalog, firmwareState } from "../../../lib/firmware";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";

const t = strings.firmware;

/**
 * A device's firmware on its page (D-121): its release and the upgrades on offer; admins
 * prepare one on the Updates page.
 */
export function FirmwareCard(props: { node: Node; admin: boolean }) {
  const navigate = useNavigate();
  const { catalog, error } = createCatalog();
  const firmware = createMemo(() => catalog()?.nodes.find((entry) => entry.node === props.node.id));
  const buildable = () => firmware()?.offers.filter((offer) => offer.profile !== null) ?? [];
  const state = () => {
    const entry = firmware();
    return entry ? firmwareState(entry, catalog()?.error != null) : "unknown";
  };
  return (
    <Card>
      <CardHeader
        title={t.card.title}
        actions={
          <Show when={props.admin && buildable().length > 0}>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => navigate(`/updates?node=${props.node.id}`)}
            >
              {t.prepare}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <RequestError text={error() ?? catalog()?.error ?? undefined} />
        <Show when={firmware()}>
          {(entry) => (
            <div class="flex items-start gap-3">
              <Icon
                icon={state() === "available" ? PackagePlus : PackageCheck}
                class={[
                  "mt-0.5 size-5 shrink-0",
                  state() === "available" ? "text-info" : "text-fg-subtle",
                ]}
              />
              <div class="min-w-0 text-sm">
                <p class="font-medium">{t.card.running(entry().version)}</p>
                <Show
                  when={entry().offers.length > 0}
                  fallback={
                    <p class="mt-0.5 text-fg-muted">
                      {entry().note ?? (state() === "current" ? t.card.none : "")}
                    </p>
                  }
                >
                  <ul class="mt-1 flex flex-col gap-1">
                    <For each={entry().offers}>
                      {(offer) => (
                        <li class="flex flex-wrap items-center gap-x-2">
                          <span class="tabular-nums">{offer.release}</span>
                          <span class="text-fg-muted text-xs">
                            {offer.profile === null
                              ? t.noImage
                              : (t.kind[offer.kind] ?? offer.kind)}
                          </span>
                          <a
                            href={offer.notes}
                            target="_blank"
                            rel="noopener noreferrer"
                            class="inline-flex items-center gap-1 text-accent text-xs hover:underline"
                          >
                            {t.notes}
                            <Icon icon={ExternalLink} class="size-3" />
                          </a>
                        </li>
                      )}
                    </For>
                  </ul>
                </Show>
              </div>
            </div>
          )}
        </Show>
      </div>
    </Card>
  );
}
