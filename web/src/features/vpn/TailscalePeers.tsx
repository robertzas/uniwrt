import { For, Show } from "solid-js";
import type { TailscaleFacts } from "../../api/gen/TailscaleFacts";
import type { TailscalePeerFacts } from "../../api/gen/TailscalePeerFacts";
import { formatBytes, formatDuration } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Badge, StatusDot } from "../../ui/Badge";
import { Card, CardHeader } from "../../ui/Card";

const t = strings.vpn.tailscale.peers;

/** A tailnet peer's state for people: how it is reached while online, else when it was seen. */
export function peerText(peer: TailscalePeerFacts, now: number): string {
  if (peer.online) {
    if (peer.direct) return t.direct(peer.direct);
    if (peer.relay) return t.relay(peer.relay);
    return t.online;
  }
  if (peer.last_seen !== null)
    return t.seen(formatDuration(Math.max(0, now - peer.last_seen) / 1000));
  return t.offline;
}

/** The tailnet's other machines as the gateway's `tailscaled` reports them, online first. */
export function TailscalePeers(props: { daemon: TailscaleFacts }) {
  return (
    <Card aria-label={t.title}>
      <CardHeader title={t.title} description={t.description} />
      <Show
        when={props.daemon.peers.length > 0}
        fallback={<p class="px-4 pb-4 text-fg-muted text-sm">{t.none}</p>}
      >
        <ul class="divide-y divide-edge border-edge border-t">
          <For each={props.daemon.peers}>
            {(peer) => (
              <li
                class="flex flex-wrap items-center gap-x-4 gap-y-1 px-4 py-2.5"
                aria-label={peer.name}
              >
                <div class="min-w-0 flex-1">
                  <p class="flex items-center gap-2 truncate font-medium text-sm">
                    {peer.name}
                    <Show when={peer.exit_node_option}>
                      <Badge>{t.exitNode}</Badge>
                    </Show>
                  </p>
                  <p class="truncate text-fg-muted text-xs">
                    {[peer.addresses[0], peer.os].filter(Boolean).join(" · ")}
                  </p>
                </div>
                <div class="flex flex-col items-end gap-0.5 text-xs">
                  <p class="flex items-center gap-2">
                    <StatusDot tone={peer.online ? "ok" : "neutral"} />
                    <span>{peerText(peer, Date.now())}</span>
                  </p>
                  <Show when={peer.rx_bytes + peer.tx_bytes > 0}>
                    <p class="text-fg-subtle">
                      {strings.vpn.wireguard.traffic(
                        formatBytes(peer.rx_bytes),
                        formatBytes(peer.tx_bytes),
                      )}
                    </p>
                  </Show>
                </div>
              </li>
            )}
          </For>
        </ul>
        <Show when={props.daemon.peer_count > props.daemon.peers.length}>
          <p class="border-edge border-t px-4 py-2 text-fg-subtle text-xs">
            {t.more(props.daemon.peers.length, props.daemon.peer_count)}
          </p>
        </Show>
      </Show>
    </Card>
  );
}
