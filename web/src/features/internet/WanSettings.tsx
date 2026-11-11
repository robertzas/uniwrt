import { ExternalLink } from "lucide";
import { For, Show } from "solid-js";
import { strings } from "../../lib/strings";
import { Icon } from "../../ui/Icon";
import { comboError } from "./check";
import { AdvancedFields, FieldGrid } from "./WanFields";
import type { WanForm } from "./wanForm";

const w = strings.internet.wizard;

/**
 * The connection's settings (wan.md, Wizard flow): the preset's notes and sources, only the
 * fields it prompts for, and every other field under Advanced.
 */
export function WanSettings(props: { form: WanForm; storedPassword: boolean }) {
  const form = () => props.form;
  return (
    <div class="flex flex-col gap-4">
      <Show when={form().preset()} fallback={<p class="font-medium">{strings.internet.custom}</p>}>
        {(p) => (
          <div>
            <p class="font-medium">
              {p().name} <span class="font-normal text-fg-muted">· {p().variant}</span>
            </p>
            <Show when={p().notes}>
              <p class="mt-1 text-fg-muted text-sm">{p().notes}</p>
            </Show>
            <Show when={p().ipv6.mode === "auto"}>
              <p class="mt-1 text-fg-muted text-sm">{w.auto}</p>
            </Show>
            <Show when={p().sources.length > 0}>
              <p class="mt-1.5 flex flex-wrap gap-x-3 gap-y-1 text-xs">
                <span class="text-fg-subtle">{w.sources}</span>
                <For each={p().sources}>
                  {(url) => (
                    <a
                      href={url}
                      target="_blank"
                      rel="noreferrer noopener"
                      class="inline-flex items-center gap-1 text-accent hover:underline"
                    >
                      {new URL(url).hostname}
                      <Icon icon={ExternalLink} class="size-3" />
                    </a>
                  )}
                </For>
              </p>
            </Show>
          </div>
        )}
      </Show>
      <Show when={form().prompted().length > 0}>
        <FieldGrid
          fields={form().prompted()}
          draft={form().draft()}
          errors={form().errors()}
          storedPassword={props.storedPassword}
          onChange={form().change}
        />
      </Show>
      <details
        open={form().advanced()}
        onToggle={(event) => form().setAdvanced(event.currentTarget.open)}
        class="rounded-control border border-edge"
      >
        <summary class="cursor-pointer select-none px-3 py-2 font-medium text-sm">
          {w.advanced} <span class="font-normal text-fg-subtle">· {w.advancedHint}</span>
        </summary>
        <div class="border-edge border-t p-3">
          <AdvancedFields
            draft={form().draft()}
            fields={form().rest()}
            errors={form().errors()}
            combo={comboError(form().draft())}
            storedPassword={props.storedPassword}
            onChange={form().change}
          />
        </div>
      </details>
    </div>
  );
}
