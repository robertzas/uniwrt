import { For } from "solid-js";
import type { Purpose } from "../../api/gen/Purpose";
import { strings } from "../../lib/strings";

const t = strings.networks.editor.applyNote;

/**
 * What applying a new network does (network.md, Networks; the owner's choice of guidance over
 * new checks, D-157): nothing until applied, the fleet's automatic rollback, a first VLAN's
 * bridge filtering, the tagged VLAN a switch must pass, and a management network's move.
 */
export function ApplyNote(props: { purpose: Purpose; vlan: string; firstVlan: boolean }) {
  const lines = () => [
    t.saved,
    t.rollback,
    ...(props.vlan.trim() !== "" && props.firstVlan ? [t.firstVlan] : []),
    ...(props.vlan.trim() !== "" ? [t.tagged(props.vlan.trim())] : []),
    ...(props.purpose === "mgmt" ? [t.mgmt] : []),
  ];
  return (
    <section
      aria-label={t.title}
      class="rounded-control border border-edge bg-raised px-3.5 py-3 text-fg-muted text-xs"
    >
      <h3 class="font-medium text-fg">{t.title}</h3>
      <ul class="mt-1.5 flex list-disc flex-col gap-1 pl-4">
        <For each={lines()}>{(line) => <li>{line}</li>}</For>
      </ul>
    </section>
  );
}
