import type { JSX } from "@solidjs/web";
import { For, Show } from "solid-js";
import type { EncryptedResolver } from "../../../../api/gen/EncryptedResolver";
import type { ServiceFacts } from "../../../../api/gen/ServiceFacts";
import type { Upstream } from "../../../../api/gen/Upstream";
import { strings } from "../../../../lib/strings";
import { Badge, StatusDot } from "../../../../ui/Badge";
import { resolverName } from "./draft";
import { DOT_PORT } from "./presets";

const t = strings.networks.upstream;

/** The layers as saved, in the order they are asked, and the proxies the gateway reports. */
export function UpstreamSummary(props: { upstream: Upstream; services: ServiceFacts | undefined }) {
  const strict = () => props.upstream.strict && props.upstream.encrypted.length > 0;
  const doh = () => props.upstream.encrypted.filter((r) => r.protocol === "doh").length;
  const dot = () => props.upstream.encrypted.some((r) => r.protocol === "dot");
  return (
    <div class="flex flex-col gap-3">
      <ol class="flex flex-col gap-3" aria-label={t.layers}>
        <Show when={props.upstream.encrypted.length > 0}>
          <Layer title={t.encrypted} badge={strict() ? t.strict : undefined} leftOut={false}>
            <dl class="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1.5 text-sm">
              <For each={props.upstream.encrypted}>
                {(resolver) => (
                  <>
                    <dt class="flex items-center gap-2 text-xs leading-5">
                      <span>{resolverName(resolver)}</span>
                      <span class="text-fg-subtle">{t.short[resolver.protocol]}</span>
                    </dt>
                    <dd class="min-w-0 truncate font-mono text-fg-muted text-xs leading-5">
                      {detail(resolver)}
                    </dd>
                  </>
                )}
              </For>
            </dl>
          </Layer>
        </Show>
        <Show when={props.upstream.servers.length > 0}>
          <Layer title={t.servers} leftOut={strict()}>
            <ul class="flex flex-wrap gap-x-4 gap-y-1 font-mono text-xs" aria-label={t.servers}>
              <For each={props.upstream.servers}>{(server) => <li>{server}</li>}</For>
            </ul>
          </Layer>
        </Show>
        <Show when={props.upstream.isp}>
          <Layer title={t.ispLayer} leftOut={strict()}>
            <p class="text-fg-muted text-sm">{t.isp}</p>
          </Layer>
        </Show>
      </ol>
      <Show when={props.services}>
        {(services) => (
          <>
            <Show when={doh() > 0}>
              <Running
                ok={(services().doh_instances ?? 0) >= doh()}
                text={
                  (services().doh_instances ?? 0) === 0
                    ? t.noProxy
                    : t.proxies(services().doh_instances ?? 0, doh())
                }
              />
            </Show>
            <Show when={dot()}>
              <Running
                ok={(services().dot_instances ?? 0) > 0}
                text={t.stubby((services().dot_instances ?? 0) > 0)}
              />
            </Show>
          </>
        )}
      </Show>
    </div>
  );
}

/** What a resolver's line says after its name. */
function detail(resolver: EncryptedResolver): string {
  if (resolver.protocol === "doh") {
    const bootstrap = resolver.bootstrap.join(", ");
    return bootstrap ? `${resolver.url} · ${t.bootstrapLabel} ${bootstrap}` : resolver.url;
  }
  const host = resolver.port === DOT_PORT ? resolver.name : `${resolver.name}:${resolver.port}`;
  const parts = [host, resolver.addresses.join(", ")];
  if (resolver.spki.length > 0) parts.push(t.pinsLabel(resolver.spki.length));
  return parts.join(" · ");
}

function Layer(props: {
  title: string;
  badge?: string | undefined;
  leftOut: boolean;
  children: JSX.Element;
}) {
  return (
    <li class={["flex flex-col gap-1.5", { "opacity-60": props.leftOut }]}>
      <div class="flex flex-wrap items-center gap-2">
        <p class="font-medium text-fg-muted text-xs">{props.title}</p>
        <Show when={props.badge}>{(badge) => <Badge tone="ok">{badge()}</Badge>}</Show>
        <Show when={props.leftOut}>
          <span class="text-fg-subtle text-xs">{t.leftOut}</span>
        </Show>
      </div>
      {props.children}
    </li>
  );
}

function Running(props: { ok: boolean; text: string }) {
  return (
    <p class="flex items-center gap-2 text-xs" role="status">
      <StatusDot tone={props.ok ? "ok" : "warn"} />
      {props.text}
    </p>
  );
}
