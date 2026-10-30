import { createEffect } from "solid-js";
import { strings } from "../lib/strings";
import { EmptyState } from "../ui/EmptyState";
import type { NavItem } from "./nav";

/** The placeholder for a section whose feature has not landed yet. */
export function SectionPage(props: { item: NavItem }) {
  createEffect(
    () => props.item.label,
    (label) => {
      document.title = `${label} · ${strings.appName}`;
    },
  );
  return (
    <EmptyState
      icon={props.item.icon}
      title={props.item.label}
      description={props.item.description}
    >
      <p class="mt-6 rounded-full border border-edge px-3 py-1 text-fg-subtle text-xs">
        {strings.page.soon}
      </p>
    </EmptyState>
  );
}
