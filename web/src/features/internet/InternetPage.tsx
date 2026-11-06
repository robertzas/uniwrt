import { useNavigate } from "@solidjs/router";
import { useQuery } from "@tanstack/solid-query";
import { Eye, Globe, TriangleAlert } from "lucide";
import { createEffect, createSignal, For, Show, useContext } from "solid-js";
import { api } from "../../lib/api";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { RequestError } from "../../ui/JobOutcome";
import { rowsOf } from "./summary";
import { WanWizard } from "./WanWizard";

const t = strings.internet;

/**
 * `/internet` (ui.md, Information architecture; wan.md): the site's WAN in words and, for
 * admins, the wizard that changes it. The live connection is the dashboard's Internet card.
 */
export function InternetPage() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const navigate = useNavigate();
  const admin = () => session.user.role === "admin";
  const site = useQuery(() => ({
    queryKey: ["site", live.config.head?.id],
    queryFn: ({ signal }) => api.site.get(signal),
  }));
  const presets = useQuery(() => ({
    queryKey: ["isp-presets"],
    queryFn: ({ signal }) => api.wan.presets(signal),
    staleTime: Number.POSITIVE_INFINITY,
  }));
  const gateway = () => live.fleet.nodes.find((node) => node.role === "gateway");
  const [wizard, setWizard] = createSignal(false);
  /** The gateway, when its `network` package isn't managed: applies leave its WAN alone. */
  const unmanaged = () => {
    const node = gateway();
    const nodes = site.data?.config.nodes;
    if (!node || !nodes) return undefined;
    const managed = nodes.find((config) => config.node === node.id)?.managed ?? [];
    return managed.includes("network") ? undefined : node;
  };

  createEffect(
    () => t.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  return (
    <div class="mx-auto flex max-w-3xl flex-col gap-4">
      <header>
        <h1 class="font-semibold text-2xl tracking-tight">{t.title}</h1>
        <p class="mt-1 text-fg-muted text-sm">{t.description}</p>
      </header>
      <Show when={live.fleet.ready && !gateway()}>
        <EmptyState icon={Globe} title={t.noGateway.title} description={t.noGateway.description} />
      </Show>
      <Show when={gateway()?.mode === "observe" ? gateway() : undefined}>
        {(node) => (
          <div class="flex flex-wrap items-center gap-3 rounded-control border border-info/40 bg-info/10 px-3 py-2.5 text-sm">
            <Icon icon={Eye} class="size-4 shrink-0 text-info" />
            <p class="min-w-0 flex-1">{t.observed(node().name)}</p>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => navigate(`/devices/${node().id}?tab=config`)}
            >
              {t.manage}
            </Button>
          </div>
        )}
      </Show>
      <Show when={unmanaged()}>
        {(node) => (
          <div class="flex flex-wrap items-center gap-3 rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5 text-sm">
            <Icon icon={TriangleAlert} class="size-4 shrink-0 text-warn" />
            <p class="min-w-0 flex-1">{t.unmanagedNetwork(node().name)}</p>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => navigate(`/devices/${node().id}?tab=config`)}
            >
              {t.manage}
            </Button>
          </div>
        )}
      </Show>
      <Card aria-label={t.connection}>
        <CardHeader
          title={t.connection}
          actions={
            <Show when={admin() && site.isSuccess && presets.isSuccess}>
              <Button size="sm" onClick={() => setWizard(true)}>
                {site.data?.config.wan.preset === null ? t.setUp : t.change}
              </Button>
            </Show>
          }
        />
        <div class="px-4 pb-4">
          <Show
            when={site.isSuccess && presets.isSuccess}
            fallback={<p class="text-fg-muted text-sm">{t.loading}</p>}
          >
            <dl class="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
              <For each={rowsOf(site.data?.config.wan ?? (undefined as never), presets.data ?? [])}>
                {([label, value]) => (
                  <>
                    <dt class="text-fg-muted">{label}</dt>
                    <dd class="min-w-0 break-words">{value}</dd>
                  </>
                )}
              </For>
            </dl>
          </Show>
          <RequestError text={site.error?.message ?? presets.error?.message} />
          <Show when={!admin()}>
            <p class="mt-3 text-fg-subtle text-sm">{t.adminOnly}</p>
          </Show>
        </div>
      </Card>
      <Show
        when={
          wizard() && site.data && presets.data
            ? { site: site.data, presets: presets.data }
            : undefined
        }
      >
        {(data) => (
          <WanWizard
            site={data().site}
            presets={data().presets}
            gateway={gateway()}
            onClose={() => setWizard(false)}
          />
        )}
      </Show>
    </div>
  );
}
