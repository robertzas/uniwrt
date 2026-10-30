import type { JSX } from "@solidjs/web";

/** A keyboard key or shortcut, e.g. <Kbd>⌘K</Kbd>. */
export function Kbd(props: { children: JSX.Element; class?: string }) {
  return (
    <kbd
      class={[
        "inline-flex h-5 items-center rounded border border-edge px-1.5 font-medium font-mono text-[11px] text-fg-subtle",
        props.class,
      ]}
    >
      {props.children}
    </kbd>
  );
}
