import { Download, ShieldCheck } from "lucide";
import { createSignal, Match, Show, Switch } from "solid-js";
import type { TlsInfo } from "../../api/gen/TlsInfo";
import { api, CA_URL } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Icon } from "../../ui/Icon";

const t = strings.auth.trust;

/**
 * Under the signed-out screens (D-151, M9.5d): the local CA to download and its fingerprint, read
 * once asked for, where the browser's certificate warning is first met. Both routes are public.
 */
export function TrustCa() {
  const [open, setOpen] = createSignal(false);
  const [info, setInfo] = createSignal<TlsInfo>();
  const [failed, setFailed] = createSignal(false);
  const toggle = () => {
    const next = !open();
    setOpen(next);
    if (next && !info()) {
      setFailed(false);
      api.tls().then(setInfo, () => setFailed(true));
    }
  };
  return (
    <div class="mt-6 w-full max-w-sm text-center">
      <button
        type="button"
        aria-expanded={open() ? "true" : "false"}
        class="inline-flex items-center gap-1.5 text-fg-muted text-xs hover:text-fg"
        onClick={toggle}
      >
        <Icon icon={ShieldCheck} class="size-3.5" />
        {t.link}
      </button>
      <Show when={open()}>
        <div class="mt-3 rounded-card border border-edge bg-surface px-4 py-3 text-left text-xs">
          <p class="text-fg-muted leading-relaxed">{t.body}</p>
          <p class="mt-3 font-medium text-fg">{t.fingerprint}</p>
          <Switch fallback={<p class="mt-1 text-fg-subtle">{t.loading}</p>}>
            <Match when={info()}>
              {(found) => (
                <code class="mt-1 block select-all break-all font-mono">{found().ca_sha256}</code>
              )}
            </Match>
            <Match when={failed()}>
              <p role="alert" class="mt-1 text-crit">
                {t.failed}
              </p>
            </Match>
          </Switch>
          <a
            href={CA_URL}
            download="uniwrt-ca.crt"
            class="mt-3 inline-flex items-center gap-1.5 font-medium text-accent hover:underline"
          >
            <Icon icon={Download} class="size-3.5" />
            {t.download}
          </a>
        </div>
      </Show>
    </div>
  );
}
