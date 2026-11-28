import { useQuery } from "@tanstack/solid-query";
import { Show, useContext } from "solid-js";
import { api } from "../../../lib/api";
import { LiveContext } from "../../../lib/live";
import { strings } from "../../../lib/strings";

const t = strings.adoptionPort;

/**
 * The site's adoption port, or how to set one up, under the inbox's header: a link to the
 * gateway's Ports tab, where it is set (D-155). Nothing while there is no gateway.
 */
export function PortNote() {
  const live = useContext(LiveContext);
  const site = useQuery(() => ({
    queryKey: ["site", live.config.head?.id],
    queryFn: ({ signal }) => api.site.get(signal),
  }));
  const gateway = () => live.fleet.nodes.find((node) => node.role === "gateway");
  /** The site's adoption port; tests' stubs may answer the site's read with another shape. */
  const port = () => {
    const config = site.data?.config;
    return config ? config.adoption_port : undefined;
  };
  const ready = () => site.isSuccess && site.data?.config !== undefined;
  return (
    <Show when={ready() && gateway()}>
      {(node) => {
        const link = (text: string) => (
          <a href={`/devices/${node().id}?tab=ports`} class="text-accent hover:underline">
            {text}
          </a>
        );
        return (
          <p class="px-4 pb-3 text-fg-muted text-sm">
            <Show
              when={port()}
              fallback={
                <>
                  {link(t.setUp)} {t.setUpHint}
                </>
              }
            >
              {(port) => (
                <>
                  {t.on(port().port)} · {link(t.open)}
                </>
              )}
            </Show>
          </p>
        );
      }}
    </Show>
  );
}
