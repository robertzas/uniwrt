import { KeyRound, Pencil, Plus, QrCode as QrIcon, TriangleAlert } from "lucide";
import { createSignal, For, Show } from "solid-js";
import type { SiteView } from "../../api/gen/SiteView";
import type { VpnPeer } from "../../api/gen/VpnPeer";
import type { VpnStatus } from "../../api/gen/VpnStatus";
import type { WireGuard } from "../../api/gen/WireGuard";
import { strings } from "../../lib/strings";
import { Badge, StatusDot } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { PeerConfigDialog } from "./PeerConfigDialog";
import { PeerEditor } from "./PeerEditor";
import { MAX_PEERS, peerState, roleText } from "./wireguard";

const t = strings.vpn.wireguard;

/** What the editor is open on: a peer, or a new one, with the document as it opened. */
type Editing = { peer: VpnPeer | undefined; view: SiteView };

/**
 * The server's peers (network.md, VPN; D-140): each with its role, tunnel address and what the
 * gateway reports (handshake, traffic, where it dials from). Admins add and edit peers, and
 * reveal a peer's configuration.
 */
export function PeersCard(props: {
  view: SiteView;
  server: WireGuard;
  admin: boolean;
  status: VpnStatus | undefined;
}) {
  const [editing, setEditing] = createSignal<Editing>();
  const [showing, setShowing] = createSignal<VpnPeer>();
  const peers = () => props.server.peers;
  const warnings = (id: string) =>
    props.view.issues
      .filter((i) => i.path.startsWith(`vpn/wireguard/peers/${id}/`))
      .map((i) => i.message);

  return (
    <Card aria-label={t.peers.title}>
      <CardHeader
        title={t.peers.title}
        description={t.peers.description}
        actions={
          <Show when={props.admin && peers().length < MAX_PEERS}>
            <Button
              size="sm"
              icon={Plus}
              onClick={() => setEditing({ peer: undefined, view: props.view })}
            >
              {t.peers.add}
            </Button>
          </Show>
        }
      />
      <Show
        when={peers().length > 0}
        fallback={<p class="px-4 pb-4 text-fg-muted text-sm">{t.peers.none}</p>}
      >
        <ul class="divide-y divide-edge border-edge border-t">
          <For each={peers()}>
            {(peer) => (
              <Row
                peer={peer}
                status={props.status}
                warnings={warnings(peer.id)}
                admin={props.admin}
                onEdit={() => setEditing({ peer, view: props.view })}
                onShow={() => setShowing(peer)}
              />
            )}
          </For>
        </ul>
      </Show>
      <Show when={editing()}>
        {(open) => (
          <PeerEditor
            view={open().view}
            server={open().view.config.vpn.wireguard ?? props.server}
            peer={open().peer}
            onShow={(peer) => {
              setEditing(undefined);
              setShowing(peer);
            }}
            onClose={() => setEditing(undefined)}
          />
        )}
      </Show>
      <Show when={showing()}>
        {(peer) => <PeerConfigDialog peer={peer()} onClose={() => setShowing(undefined)} />}
      </Show>
    </Card>
  );
}

function Row(props: {
  peer: VpnPeer;
  status: VpnStatus | undefined;
  warnings: string[];
  admin: boolean;
  onEdit: () => void;
  onShow: () => void;
}) {
  const state = () =>
    peerState(
      props.peer,
      props.status?.peers.find((p) => p.id === props.peer.id),
      props.status,
      Date.now(),
    );
  const own = () => props.peer.private_key === null && props.peer.public_key !== "";
  return (
    <li class="flex flex-col gap-1 px-4 py-3" aria-label={props.peer.name}>
      <div class="flex flex-wrap items-center gap-x-4 gap-y-1">
        <div class="min-w-0 flex-1">
          <p class="flex items-center gap-2 truncate font-medium text-sm">
            {props.peer.name}
            <Show when={own()}>
              <Badge icon={KeyRound}>{t.peers.ownKey}</Badge>
            </Show>
          </p>
          <p class="text-fg-muted text-xs">
            <span class="font-mono">{props.peer.address}</span> · {roleText(props.peer)}
          </p>
        </div>
        <div class="flex flex-col items-end gap-0.5 text-xs">
          <p class="flex items-center gap-2">
            <StatusDot tone={state().tone} />
            <span>{state().text}</span>
          </p>
          <Show when={state().traffic || state().endpoint}>
            <p class="text-fg-subtle">
              {[state().endpoint, state().traffic].filter(Boolean).join(" · ")}
            </p>
          </Show>
        </div>
        <Show when={props.admin}>
          <div class="flex gap-1">
            <Button
              size="sm"
              variant="ghost"
              icon={QrIcon}
              aria-label={t.peers.config(props.peer.name)}
              title={t.peers.config(props.peer.name)}
              onClick={() => props.onShow()}
            />
            <Button
              size="sm"
              variant="ghost"
              icon={Pencil}
              aria-label={t.peers.edit(props.peer.name)}
              title={t.peers.edit(props.peer.name)}
              onClick={() => props.onEdit()}
            />
          </div>
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
