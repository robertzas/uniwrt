import { Ban, Check } from "lucide";
import { createSignal, Show } from "solid-js";
import type { Blocking } from "../../../api/gen/Blocking";
import { errorText } from "../../../lib/api";
import { saveSite } from "../../../lib/site";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { canList, listedOf, withListed } from "./dnsStats";

const t = strings.networks.queries;

/** Saves domains to the allow or deny list, one revision each, and keeps the last outcome. */
export interface Listing {
  busy: (domain: string) => boolean;
  save: (domain: string, list: "allow" | "deny") => Promise<void>;
  /** The revision the last save made. */
  saved: () => number | undefined;
  failure: () => string | undefined;
}

export function createListing(): Listing {
  const [busy, setBusy] = createSignal<ReadonlySet<string>>(new Set());
  const [saved, setSaved] = createSignal<number>();
  const [failure, setFailure] = createSignal<string>();
  const mark = (domain: string, on: boolean) =>
    setBusy((now) => {
      const next = new Set(now);
      if (on) next.add(domain);
      else next.delete(domain);
      return next;
    });
  return {
    busy: (domain) => busy().has(domain),
    saved,
    failure,
    save: async (domain, list) => {
      mark(domain, true);
      setFailure(undefined);
      try {
        const view = await saveSite(
          (config) => {
            const next = withListed(config.services.blocking, domain, list);
            if ("error" in next) throw new Error(next.error);
            return { ...config, services: { ...config.services, blocking: next.blocking } };
          },
          list === "allow" ? t.allowMessage(domain) : t.denyMessage(domain),
        );
        setSaved(view.revision.id);
      } catch (err) {
        setFailure(errorText(err));
      } finally {
        mark(domain, false);
      }
    },
  };
}

/**
 * What a domain's row offers: whether the latest revision already allows or blocks it, else (for
 * admins) Allow on a blocked domain and Block on an answered one: an icon button that shows when
 * its row (a `group`) is hovered or it has focus, and always where nothing hovers (touch).
 */
export function DomainAction(props: {
  domain: string;
  /** The filter blocked it: the action allows it. */
  blocked: boolean;
  blocking: Blocking;
  admin: boolean;
  listing: Listing;
}) {
  const listed = () => listedOf(props.blocking, props.domain);
  const label = () => (props.blocked ? t.allowDomain(props.domain) : t.blockDomain(props.domain));
  return (
    <Show
      when={listed()}
      fallback={
        <Show when={props.admin && canList(props.domain)}>
          <Button
            size="sm"
            variant="ghost"
            icon={props.blocked ? Check : Ban}
            disabled={props.listing.busy(props.domain)}
            aria-label={label()}
            title={label()}
            class="focus-visible:opacity-100 group-hover:opacity-100 [@media(hover:hover)]:opacity-0"
            onClick={() => void props.listing.save(props.domain, props.blocked ? "allow" : "deny")}
          />
        </Show>
      }
    >
      {(listed) => (
        <Badge tone={listed() === "allowed" ? "ok" : "crit"}>
          {listed() === "allowed" ? t.allowed : t.denied}
        </Badge>
      )}
    </Show>
  );
}
