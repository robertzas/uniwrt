import type { JSX } from "@solidjs/web";
import {
  elementScroll,
  observeElementOffset,
  observeElementRect,
  type VirtualItem,
  Virtualizer,
} from "@tanstack/virtual-core";
import { createEffect, createSignal, For, onSettled, Show, untrack } from "solid-js";
import type { LogRecord } from "../../api/gen/LogRecord";
import { strings } from "../../lib/strings";

/** Above this many entries only those in view are rendered (ui.md: 200). */
const VIRTUALIZE_ABOVE = 200;
/** Within this many pixels of the top, the reader follows new entries. */
const TOP = 8;

export interface LogListProps {
  records: readonly LogRecord[];
  row: (record: LogRecord) => JSX.Element;
  /** The reader scrolled to the newest entries (`true`) or away from them. */
  onFollow: (following: boolean) => void;
  /** The end of the list came into view: read older entries. */
  onEnd: () => void;
  /** Hands the page a way to scroll back to the newest entries. */
  controls?: (controls: { toTop: () => void }) => void;
  /** What follows the last entry: loading, the end, or a button for more. */
  footer: JSX.Element;
  empty: JSX.Element;
}

/**
 * The entries, newest first, in a scroll box of their own. Scrolling down stops following the
 * newest; the end of the list asks for older entries.
 */
export function LogList(props: LogListProps) {
  const [scroller, setScroller] = createSignal<HTMLDivElement>();
  const [sentinel, setSentinel] = createSignal<HTMLDivElement>();
  const virtual = () => props.records.length > VIRTUALIZE_ABOVE;

  onSettled(() => {
    untrack(() => props.controls)?.({
      toTop: () => {
        const element = scroller();
        if (element) element.scrollTop = 0;
      },
    });
  });
  createEffect(
    () => [sentinel(), scroller()] as const,
    ([element, root]) => {
      if (!element || !root || typeof IntersectionObserver === "undefined") return;
      const observer = new IntersectionObserver(
        (entries) => {
          if (entries.some((entry) => entry.isIntersecting)) props.onEnd();
        },
        { root, rootMargin: "400px 0px" },
      );
      observer.observe(element);
      return () => observer.disconnect();
    },
  );

  return (
    <div
      ref={setScroller}
      onScroll={(event) => props.onFollow(event.currentTarget.scrollTop <= TOP)}
      class="relative h-[max(24rem,calc(100dvh-24rem))] overflow-auto overscroll-contain [overflow-anchor:none]"
    >
      <Show when={props.records.length > 0} fallback={props.empty}>
        <Show when={virtual()} fallback={<PlainRows {...props} />}>
          <VirtualRows {...props} scroller={scroller} />
        </Show>
        <div ref={setSentinel}>{props.footer}</div>
      </Show>
    </div>
  );
}

function PlainRows(props: LogListProps) {
  return (
    <ul aria-label={strings.logs.list}>
      <For each={props.records} keyed={(record) => record.id}>
        {(record) => <li>{props.row(record())}</li>}
      </For>
    </ul>
  );
}

function VirtualRows(props: LogListProps & { scroller: () => HTMLDivElement | undefined }) {
  const [items, setItems] = createSignal<VirtualItem[]>([]);
  const [total, setTotal] = createSignal(0);
  const sync = (instance: Virtualizer<HTMLDivElement, HTMLLIElement>) => {
    setItems(instance.getVirtualItems());
    setTotal(instance.getTotalSize());
  };
  const virtualizer = new Virtualizer<HTMLDivElement, HTMLLIElement>({
    count: untrack(() => props.records.length),
    getScrollElement: () => props.scroller() ?? null,
    estimateSize: () => 31,
    getItemKey: (index) => props.records[index]?.id ?? index,
    overscan: 15,
    observeElementRect,
    observeElementOffset,
    scrollToFn: elementScroll,
    onChange: sync,
  });
  // Mount once the scroll box is in the DOM: onSettled can run before refs are set (D-045).
  createEffect(props.scroller, (element) => {
    if (!element) return;
    const cleanup = virtualizer._didMount();
    virtualizer._willUpdate();
    sync(virtualizer);
    return cleanup;
  });
  createEffect(
    () => props.records.length,
    (count) => {
      virtualizer.setOptions({ ...virtualizer.options, count });
      virtualizer._willUpdate();
      sync(virtualizer);
    },
    { defer: true },
  );
  const before = () => items()[0]?.start ?? 0;
  const after = () => total() - (items().at(-1)?.end ?? 0);
  return (
    <ul
      aria-label={strings.logs.list}
      style={{ "padding-top": `${before()}px`, "padding-bottom": `${after()}px` }}
    >
      <For each={items()} keyed={(item) => item.key}>
        {(item) => (
          <Show when={props.records[item().index]}>
            {(record) => (
              <li
                data-index={item().index}
                aria-setsize={props.records.length}
                aria-posinset={item().index + 1}
                ref={(el) => virtualizer.measureElement(el)}
              >
                {props.row(record())}
              </li>
            )}
          </Show>
        )}
      </For>
    </ul>
  );
}
