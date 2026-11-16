import { useNavigate } from "@solidjs/router";
import { ExternalLink, PackageCheck, PackagePlus } from "lucide";
import { createMemo, For, Show } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import { createCatalog, firmwareState, hasImages } from "../../../lib/firmware";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";

const t = strings.firmware;

/**
 * A device's firmware on its page (D-121): its release and the upgrades on offer; admins
 * prepare one on the Updates page (also with an uploaded image made for it, D-122).
 */
export function FirmwareCard(props: { node: Node; admin: boolean }) {
  const navigate = useNavigate();
  const { catalog, error } = createCatalog();
  const firmware = createMemo(() => catalog()?.nodes.find((entry) => entry.node === props.node.id));
  const preparable = () => {
    const entry = firmware();
    return entry !== undefined && hasImages(entry);
  };
  const state = () => {
    const entry = firmware();
    return entry ? firmwareState(entry, catalog()?.error != null) : "unknown";
  };
  return (
    <Card>
      <CardHeader
        title={t.card.title}
        actions={
          <Show when={props.admin && preparable()}>
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
                icon={state() === "current" || state() === "unknown" ? PackageCheck : PackagePlus}
                class={[
                  "mt-0.5 size-5 shrink-0",
                  state() === "current" || state() === "unknown" ? "text-fg-subtle" : "text-info",
                ]}
              />
              <div class="min-w-0 text-sm">
                <p class="font-medium">{t.card.running(entry().version)}</p>
                <Show when={entry().releases.length > 0}>
                  <p class="mt-0.5 text-fg-muted text-xs">{t.source.github(entry().repo ?? "")}</p>
                  <ul class="mt-1 flex flex-col gap-1">
                    <For each={entry().releases}>
                      {(release) => (
                        <li class="flex flex-wrap items-center gap-x-2">
                          <span>{release.name}</span>
                          <Show when={release.prerelease}>
                            <span class="text-fg-muted text-xs">{t.prerelease}</span>
                          </Show>
                          <a
                            href={release.notes}
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
                <Show
                  when={entry().releases.length === 0 && entry().offers.length > 0}
                  fallback={
                    <Show when={entry().releases.length === 0}>
                      <p class="mt-0.5 text-fg-muted">
                        {entry().note ?? (state() === "current" ? t.card.none : "")}
                      </p>
                    </Show>
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
