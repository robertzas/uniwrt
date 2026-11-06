import { createSignal, For, Show } from "solid-js";
import type { Client } from "../../api/gen/Client";
import type { ReservationSuggestion } from "../../api/gen/ReservationSuggestion";
import { ApiError, api, errorText } from "../../lib/api";
import { clientName } from "../../lib/clients";
import { hostnameOf, isHostname } from "../../lib/names";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { RequestError } from "../../ui/JobOutcome";
import { SavedRevision } from "../../ui/SavedRevision";
import { TextField } from "../../ui/TextField";

const t = strings.client.fixIp;
const IPV4 = /^\d{1,3}(\.\d{1,3}){3}$/;

/** The client's IPv4 address, which "Fix IP" keeps when it can. */
export function ipv4Of(client: Client): string | undefined {
  return client.ips.find((ip) => IPV4.test(ip));
}

/**
 * "Fix IP" (network.md, DHCP & local DNS): the address plan suggests where to reserve the
 * client's address (`GET v1/ipam/reservation`); the reservation is saved as the next revision.
 * Mounted while open.
 */
export function FixIpDialog(props: { client: Client; onClose: () => void }) {
  const ip = ipv4Of(props.client);
  const [found, setFound] = createSignal<{ suggestion: ReservationSuggestion; network: string }>();
  const [address, setAddress] = createSignal("");
  const [name, setName] = createSignal(
    hostnameOf(props.client.alias ?? props.client.hostname ?? null),
  );
  const [nameError, setNameError] = createSignal<string>();
  const [failure, setFailure] = createSignal<string>();
  const [issues, setIssues] = createSignal<string[]>([]);
  const [busy, setBusy] = createSignal(ip !== undefined);
  const [saved, setSaved] = createSignal<number>();

  if (ip) {
    Promise.all([api.ipam.reservation(ip, props.client.mac), api.site.get()])
      .then(([suggestion, site]) => {
        const network = site.config.networks.find((n) => n.id === suggestion.network);
        setFound({ suggestion, network: network?.name ?? suggestion.network });
        setAddress(suggestion.ip);
      })
      .catch((err) => setFailure(errorText(err)))
      .finally(() => setBusy(false));
  }

  const save = async () => {
    const at = found();
    if (!at) return;
    const host = name().trim();
    if (host !== "" && !isHostname(host)) {
      setNameError(strings.networks.errors.hostname);
      return;
    }
    setBusy(true);
    setFailure(undefined);
    setIssues([]);
    const reservation = { mac: props.client.mac, ip: address().trim(), name: host || null };
    try {
      const view = await saveSite(
        (config) => ({
          ...config,
          networks: config.networks.map((network) => {
            const dhcp = network.ipv4?.dhcp;
            if (network.id !== at.suggestion.network || !network.ipv4 || !dhcp) return network;
            const others = dhcp.reservations.filter((r) => r.mac !== reservation.mac);
            return {
              ...network,
              ipv4: { ...network.ipv4, dhcp: { ...dhcp, reservations: [...others, reservation] } },
            };
          }),
        }),
        t.message(clientName(props.client), reservation.ip),
      );
      setSaved(view.revision.id);
    } catch (err) {
      setFailure(errorText(err));
      if (err instanceof ApiError) setIssues(err.issues.map((issue) => issue.message));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => !open && !busy() && props.onClose()}
      title={t.title}
      description={t.description}
    >
      <div class="flex flex-col gap-4 px-5 pb-5">
        <Show when={ip} fallback={<p class="text-fg-muted text-sm">{t.noIpv4}</p>}>
          <Show when={found()} fallback={<p class="text-fg-muted text-sm">{t.loading}…</p>}>
            {(at) => (
              <Show
                when={!at().suggestion.existing}
                fallback={<p class="text-sm">{t.existing(at().suggestion.ip, at().network)}</p>}
              >
                <Show
                  when={saved()}
                  fallback={
                    <form
                      class="flex flex-col gap-3"
                      onSubmit={(event) => {
                        event.preventDefault();
                        void save();
                      }}
                    >
                      <p class="text-sm">
                        <span class="font-medium">{t.on(at().network)}</span>
                        <span class="text-fg-muted">
                          {" · "}
                          {at().suggestion.ip === ip ? t.kept : t.moved(ip ?? "")}
                        </span>
                      </p>
                      <div class="grid gap-3 sm:grid-cols-2">
                        <TextField label={t.ip} value={address()} onInput={setAddress} />
                        <TextField
                          label={t.hostname}
                          value={name()}
                          onInput={(value) => {
                            setName(value);
                            setNameError(undefined);
                          }}
                          hint={t.hostnameHint}
                          error={nameError()}
                        />
                      </div>
                      <div class="flex justify-end gap-2">
                        <Button variant="ghost" onClick={() => props.onClose()} disabled={busy()}>
                          {t.cancel}
                        </Button>
                        <Button type="submit" variant="primary" disabled={busy()}>
                          {t.save}
                        </Button>
                      </div>
                    </form>
                  }
                >
                  {(revision) => <SavedRevision revision={revision()} />}
                </Show>
              </Show>
            )}
          </Show>
        </Show>
        <RequestError text={failure()} />
        <For each={issues()}>{(message) => <p class="text-crit text-xs">{message}</p>}</For>
        <Show when={saved() || found()?.suggestion.existing || !ip || (failure() && !found())}>
          <div class="flex justify-end">
            <Button onClick={() => props.onClose()}>{t.close}</Button>
          </div>
        </Show>
      </div>
    </Dialog>
  );
}
