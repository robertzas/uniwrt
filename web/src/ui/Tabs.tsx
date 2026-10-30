import * as KTabs from "@kobalte/core/tabs";
import type { JSX } from "@solidjs/web";

/** Tabs with arrow-key navigation (Kobalte). Compose Tabs > TabList > Tab, then TabPanels. */
export function Tabs(props: {
  value?: string;
  defaultValue?: string;
  onChange?: (value: string) => void;
  "aria-label"?: string;
  children: JSX.Element;
  class?: string;
}) {
  return (
    <KTabs.Root
      value={props.value}
      defaultValue={props.defaultValue}
      onChange={props.onChange}
      aria-label={props["aria-label"]}
      class={props.class}
    >
      {props.children}
    </KTabs.Root>
  );
}

export function TabList(props: { children: JSX.Element; class?: string }) {
  return (
    <KTabs.List class={["relative flex gap-1 overflow-x-auto border-edge border-b", props.class]}>
      {props.children}
      <KTabs.Indicator class="absolute bottom-0 h-0.5 rounded-full bg-accent transition-[left,width] duration-200" />
    </KTabs.List>
  );
}

export function Tab(props: { value: string; disabled?: boolean; children: JSX.Element }) {
  return (
    <KTabs.Trigger
      value={props.value}
      disabled={props.disabled}
      class="-mb-px whitespace-nowrap px-3 py-2 font-medium text-fg-subtle text-sm outline-none transition-colors hover:text-fg focus-visible:rounded-control focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50 data-selected:text-fg"
    >
      {props.children}
    </KTabs.Trigger>
  );
}

export function TabPanel(props: { value: string; children: JSX.Element; class?: string }) {
  return (
    <KTabs.Content value={props.value} class={["pt-4 outline-none", props.class]}>
      {props.children}
    </KTabs.Content>
  );
}
