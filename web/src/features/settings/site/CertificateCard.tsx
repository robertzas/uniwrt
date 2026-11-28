import { useQuery } from "@tanstack/solid-query";
import { Copy, Download } from "lucide";
import { For, Show } from "solid-js";
import { api, CA_URL } from "../../../lib/api";
import { formatDateTime } from "../../../lib/format";
import { helpHref } from "../../../lib/help";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { showToast } from "../../../ui/Toast";

const t = strings.settings.certificate;

/** An RFC 3339 time as the UI shows it, or the text as it came when it doesn't parse. */
const when = (rfc3339: string) => {
  const ms = Date.parse(rfc3339);
  return Number.isNaN(ms) ? rfc3339 : formatDateTime(ms);
};

/**
 * Settings › Site's *HTTPS certificate* (D-151, M9.5d): the local CA to download, its fingerprint
 * to compare, and the names and expiry of the certificate it signs. For everyone: the routes are
 * public, as a device needs them before it can log in without a warning.
 */
export function CertificateCard() {
  const tls = useQuery(() => ({
    queryKey: ["tls"],
    queryFn: ({ signal }) => api.tls(signal),
    staleTime: 60_000,
  }));
  const copy = (text: string) => {
    navigator.clipboard?.writeText(text).then(
      () => showToast({ title: t.copied, tone: "ok" }),
      () => {},
    );
  };
  return (
    <Card aria-label={t.title}>
      <CardHeader
        title={t.title}
        description={t.description}
        actions={
          <a
            href={CA_URL}
            download="uniwrt-ca.crt"
            class="inline-flex h-8 items-center gap-1.5 rounded-control border border-edge bg-surface px-3 font-medium text-fg text-sm hover:bg-raised"
          >
            <Icon icon={Download} class="size-4" />
            {t.download}
          </a>
        }
      />
      <Show
        when={typeof tls.data?.ca_sha256 === "string" ? tls.data : undefined}
        fallback={
          <div class="px-4 pb-4">
            <Show when={!tls.isError}>
              <p role="status" class="text-fg-muted text-sm">
                {t.loading}…
              </p>
            </Show>
            <RequestError text={tls.error?.message} />
          </div>
        }
      >
        {(info) => (
          <dl class="grid gap-x-6 gap-y-3 px-4 pb-4 text-sm sm:grid-cols-[auto_1fr]">
            <dt class="text-fg-muted">{t.ca}</dt>
            <dd>{info().ca_name}</dd>
            <dt class="text-fg-muted">{t.fingerprint}</dt>
            <dd class="min-w-0">
              <div class="flex items-start gap-1">
                <code class="min-w-0 select-all break-all font-mono text-xs leading-5">
                  {info().ca_sha256}
                </code>
                <Button
                  size="sm"
                  variant="ghost"
                  icon={Copy}
                  aria-label={t.copy}
                  title={t.copy}
                  onClick={() => copy(info().ca_sha256)}
                />
              </div>
              <p class="mt-1 break-words text-fg-subtle text-xs">{t.fingerprintHint}</p>
            </dd>
            <dt class="text-fg-muted">{t.caExpires}</dt>
            <dd>{when(info().ca_not_after)}</dd>
            <dt class="text-fg-muted">{t.names}</dt>
            <dd>
              <ul class="flex flex-wrap gap-1.5">
                <For each={info().names ?? []}>
                  {(name) => (
                    <li class="rounded-control bg-raised px-1.5 py-0.5 font-mono text-xs">
                      {name}
                    </li>
                  )}
                </For>
              </ul>
            </dd>
            <dt class="text-fg-muted">{t.expires}</dt>
            <dd>
              {when(info().not_after)}
              <span class="text-fg-subtle"> · {t.renews}</span>
            </dd>
            <dt class="sr-only">{t.howTo}</dt>
            <dd class="sm:col-start-2">
              <a
                href={helpHref({ chapter: "install", anchor: "trust-the-controllers-certificate" })}
                class="font-medium text-accent text-xs hover:underline"
              >
                {t.howTo}
              </a>
            </dd>
          </dl>
        )}
      </Show>
    </Card>
  );
}
