import { useQuery } from "@tanstack/solid-query";
import { Hourglass } from "lucide";
import { createSignal, Show, useContext } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import type { ZtpApproval } from "../../../api/gen/ZtpApproval";
import { api, errorText } from "../../../lib/api";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";

const t = strings.device.lifecycle.awaiting;

/** How often an offline node's page looks for its reset's approval. */
const POLL_MS = 15_000;

/** The approval that keeps `node`'s record waiting for its device after a reset, if any. */
export function awaitingOf(approvals: readonly ZtpApproval[], node: Node): ZtpApproval | undefined {
  return approvals.find((approval) => approval.node === node.id);
}

/**
 * An offline node reset to its defaults (D-131): its record waits for the device, by MAC and
 * board, and this says how the device comes back: a uniwrt image calls home, a stock one is
 * found on the adoption port (if the site has one) or adopted by hand at 192.168.1.1. Admins
 * stop the wait by withdrawing the approval. Read from `v1/ztp/pending` (admins only).
 */
export function AwaitingCard(props: { node: Node }) {
  const live = useContext(LiveContext);
  const inbox = useQuery(() => ({
    queryKey: ["ztp", "inbox"],
    queryFn: ({ signal }) => api.ztp.inbox(signal),
    refetchInterval: POLL_MS,
  }));
  const approval = () =>
    inbox.isSuccess ? awaitingOf(inbox.data.approvals, props.node) : undefined;
  const site = useQuery(() => ({
    queryKey: ["site", live.config.head?.id],
    queryFn: ({ signal }) => api.site.get(signal),
    enabled: approval() !== undefined,
  }));
  const port = () => (site.isSuccess ? site.data.config.adoption_port?.port : undefined);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string>();

  const stop = async (mac: string) => {
    setBusy(true);
    setError(undefined);
    try {
      await api.ztp.withdraw(mac);
      await inbox.refetch();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Show when={approval()}>
      {(waiting) => (
        <Card class="border-info/40 px-4 py-3" aria-label={t.title}>
          <div class="flex flex-wrap items-start gap-x-4 gap-y-2">
            <Icon icon={Hourglass} class="mt-0.5 size-4 shrink-0 text-info" />
            <div class="min-w-0 flex-1 text-sm">
              <p class="font-medium">{t.title}</p>
              <p class="mt-0.5 text-fg-muted">
                {t.body(waiting().mac, waiting().board_name ?? props.node.board_name)}
              </p>
              <ul class="mt-2 flex list-disc flex-col gap-1 pl-5 text-fg-muted">
                <li>{t.image}</li>
                <Show when={port()} fallback={<li>{t.manual}</li>}>
                  {(name) => <li>{t.port(name())}</li>}
                </Show>
              </ul>
              <RequestError text={error()} />
            </div>
            <Button
              size="sm"
              variant="secondary"
              title={t.stopHint}
              disabled={busy()}
              onClick={() => void stop(waiting().mac)}
            >
              {t.stop}
            </Button>
          </div>
        </Card>
      )}
    </Show>
  );
}
