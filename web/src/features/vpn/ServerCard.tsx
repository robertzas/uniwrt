import { Copy, Pencil, Plus } from "lucide";
import { createSignal, Show } from "solid-js";
import type { SiteView } from "../../api/gen/SiteView";
import type { VpnStatus } from "../../api/gen/VpnStatus";
import type { WireGuard } from "../../api/gen/WireGuard";
import { formatDuration } from "../../lib/format";
import { strings } from "../../lib/strings";
import { StatusDot, type Tone } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { endpointText } from "./wireguard";

const t = strings.vpn.wireguard;

/** The server's state on the gateway. */
export function serverState(
  server: WireGuard,
  status: VpnStatus | undefined,
): { tone: Tone; text: string } {
  const s = t.status;
  if (!server.enabled) return { tone: "neutral", text: s.off };
  if (!status || status.collected_at === null) return { tone: "neutral", text: s.unknown };
  if (!status.online) return { tone: "warn", text: s.offline };
  if (!status.running) return { tone: "info", text: s.pending };
  if (!status.key_applied) return { tone: "info", text: s.oldKey };
  return { tone: "ok", text: s.running };
}

/**
 * The WireGuard server (network.md, VPN; D-140): whether it runs on the gateway, what peers
 * dial, the tunnel and the server's public key. Admins set it up and edit it in a dialog.
 */
export function ServerCard(props: {
  view: SiteView;
  admin: boolean;
  status: VpnStatus | undefined;
  /** Opens the server's editor, which the page holds: a first save swaps this card. */
  onEdit: () => void;
}) {
  const [copied, setCopied] = createSignal(false);
  const server = () => props.view.config.vpn.wireguard;
  const copy = async (key: string) => {
    await navigator.clipboard?.writeText(key);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <Show when={props.admin}>
            <Show
              when={server()}
              fallback={
                <Button size="sm" icon={Plus} onClick={() => props.onEdit()}>
                  {t.setUp}
                </Button>
              }
            >
              <Button size="sm" icon={Pencil} onClick={() => props.onEdit()}>
                {t.edit}
              </Button>
            </Show>
          </Show>
        }
      />
      <Show
        when={server()}
        fallback={
          <p class="px-4 pb-4 text-fg-muted text-sm">{props.admin ? t.none : t.noneViewer}</p>
        }
      >
        {(wg) => {
          const state = () => serverState(wg(), props.status);
          const collected = () => {
            const at = props.status?.collected_at;
            return at == null ? undefined : formatDuration((Date.now() - at) / 1000);
          };
          return (
            <div class="flex flex-col gap-3 border-edge border-t px-4 py-3">
              <p class="flex flex-wrap items-center gap-2 text-sm">
                <StatusDot tone={state().tone} />
                <span>{state().text}</span>
                <Show when={wg().enabled && collected()}>
                  {(ago) => <span class="text-fg-subtle text-xs">{t.collected(ago())}</span>}
                </Show>
              </p>
              <dl class="grid gap-x-6 gap-y-2 text-sm sm:grid-cols-3">
                <div class="min-w-0">
                  <dt class="text-fg-subtle text-xs">{t.endpoint}</dt>
                  <dd class="truncate font-mono">
                    {endpointText(wg(), props.view.config.services.ddns)}
                  </dd>
                </div>
                <div>
                  <dt class="text-fg-subtle text-xs">{t.tunnel}</dt>
                  <dd class="font-mono">{wg().address}</dd>
                </div>
                <div>
                  <dt class="text-fg-subtle text-xs">{t.dnsLabel}</dt>
                  <dd>{t.dns(wg().dns)}</dd>
                </div>
                <div class="min-w-0 sm:col-span-3">
                  <dt class="text-fg-subtle text-xs">{t.publicKey}</dt>
                  <dd class="flex items-center gap-1">
                    <Show
                      when={wg().public_key}
                      fallback={<span class="text-fg-muted">{t.keyPending}</span>}
                    >
                      {(key) => (
                        <>
                          <span class="min-w-0 truncate font-mono text-xs">{key()}</span>
                          <Button
                            size="sm"
                            variant="ghost"
                            icon={Copy}
                            aria-label={copied() ? t.copied : t.copyKey}
                            title={copied() ? t.copied : t.copyKey}
                            onClick={() => void copy(key())}
                          />
                        </>
                      )}
                    </Show>
                  </dd>
                </div>
              </dl>
            </div>
          );
        }}
      </Show>
    </Card>
  );
}
