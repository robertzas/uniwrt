import type { JSX } from "@solidjs/web";
import {
  elementScroll,
  observeElementOffset,
  observeElementRect,
  type VirtualItem,
  Virtualizer,
} from "@tanstack/virtual-core";
import { createEffect, createSignal, For, Show, untrack } from "solid-js";

export interface Column<T> {
  id: string;
  header: string;
  cell: (row: T) => JSX.Element;
  /** "end" right-aligns the column and sets tabular digits, for numbers. */
  align?: "start" | "end";
  /** Width and other classes for the column's cells, e.g. "w-40". */
  class?: string;
}

export interface TableProps<T> {
  /** The table's accessible name. */
  label: string;
  columns: readonly Column<T>[];
  rows: readonly T[];
  rowKey: (row: T) => string;
  /** Makes rows clickable and keyboard-activatable (Enter, Space). */
  onRowClick?: (row: T) => void;
  /** Shown instead of the body when there are no rows. */
  empty?: JSX.Element;
  /** Virtualizes the body above this many rows (ui.md: 200). */
  virtualizeAbove?: number;
  class?: string;
}

const VIRTUALIZE_ABOVE = 200;

/** A data table with a sticky header; long tables render only the rows in view. */
export function Table<T>(props: TableProps<T>) {
  const virtual = () => props.rows.length > (props.virtualizeAbove ?? VIRTUALIZE_ABOVE);
  return (
    <div
      class={[
        "relative overflow-auto",
        { "max-h-[70vh] overscroll-contain": virtual() },
        props.class,
      ]}
    >
      <table aria-label={props.label} class="w-full border-collapse text-sm">
        <thead>
          <tr>
            <For each={props.columns}>
              {(column) => (
                <th
                  scope="col"
                  class={[
                    "sticky top-0 z-[1] h-8 whitespace-nowrap border-edge border-b bg-surface px-4 font-medium text-fg-subtle text-xs glass:backdrop-blur-panel",
                    column.align === "end" ? "text-right" : "text-left",
                    column.class,
                  ]}
                >
                  {column.header}
                </th>
              )}
            </For>
          </tr>
        </thead>
        <Show
          when={props.rows.length > 0}
          fallback={
            <tbody>
              <tr>
                <td colspan={props.columns.length} class="px-4 py-10 text-center text-fg-subtle">
                  {props.empty}
                </td>
              </tr>
            </tbody>
          }
        >
          <Show when={virtual()} fallback={<PlainBody {...props} />}>
            <VirtualBody {...props} />
          </Show>
        </Show>
      </table>
    </div>
  );
}

function PlainBody<T>(props: TableProps<T>) {
  return (
    <tbody>
      <For each={props.rows} keyed={props.rowKey}>
        {(row) => <Row table={props} row={row()} />}
      </For>
    </tbody>
  );
}

function VirtualBody<T>(props: TableProps<T>) {
  const [body, setBody] = createSignal<HTMLTableSectionElement>();
  const [items, setItems] = createSignal<VirtualItem[]>([]);
  const [total, setTotal] = createSignal(0);
  const sync = (instance: Virtualizer<HTMLElement, HTMLTableRowElement>) => {
    setItems(instance.getVirtualItems());
    setTotal(instance.getTotalSize());
  };
  const virtualizer = new Virtualizer<HTMLElement, HTMLTableRowElement>({
    count: untrack(() => props.rows.length),
    getScrollElement: () => body()?.closest<HTMLElement>(".overflow-auto") ?? null,
    estimateSize: () => 36,
    getItemKey: (index) => {
      const row = props.rows[index];
      return row === undefined ? index : props.rowKey(row);
    },
    overscan: 10,
    observeElementRect,
    observeElementOffset,
    scrollToFn: elementScroll,
    onChange: sync,
  });
  // Mount once the body is in the DOM: onSettled can run before refs are set (D-045).
  createEffect(body, (element) => {
    if (!element) return;
    const cleanup = virtualizer._didMount();
    virtualizer._willUpdate();
    sync(virtualizer);
    return cleanup;
  });
  createEffect(
    () => props.rows.length,
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
    <tbody ref={setBody}>
      <Show when={before() > 0}>
        {/* biome-ignore lint/a11y/noAriaHiddenOnFocusable: a spacer row; Biome counts every tr as interactive. */}
        <tr aria-hidden="true" style={{ height: `${before()}px` }} />
      </Show>
      <For each={items()} keyed={(item) => item.key}>
        {(item) => (
          <Show when={props.rows[item().index]}>
            {(row) => (
              <Row
                table={props}
                row={row()}
                index={item().index}
                ref={(el) => virtualizer.measureElement(el)}
              />
            )}
          </Show>
        )}
      </For>
      <Show when={after() > 0}>
        {/* biome-ignore lint/a11y/noAriaHiddenOnFocusable: a spacer row; Biome counts every tr as interactive. */}
        <tr aria-hidden="true" style={{ height: `${after()}px` }} />
      </Show>
    </tbody>
  );
}

function Row<T>(props: {
  table: TableProps<T>;
  row: T;
  index?: number;
  ref?: (el: HTMLTableRowElement) => void;
}) {
  const activate = (event: KeyboardEvent) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      props.table.onRowClick?.(props.row);
    }
  };
  return (
    <tr
      ref={props.ref}
      data-index={props.index}
      tabindex={props.table.onRowClick ? 0 : undefined}
      onClick={() => props.table.onRowClick?.(props.row)}
      onKeyDown={(event) => (props.table.onRowClick ? activate(event) : undefined)}
      class={[
        "h-row border-edge border-b last:border-b-0",
        {
          "cursor-pointer outline-none hover:bg-raised/60 focus-visible:bg-raised":
            props.table.onRowClick !== undefined,
        },
      ]}
    >
      <For each={props.table.columns}>
        {(column) => (
          <td
            class={[
              "whitespace-nowrap px-4",
              column.align === "end" ? "text-right tabular-nums" : "text-left",
              column.class,
            ]}
          >
            {column.cell(props.row)}
          </td>
        )}
      </For>
    </tr>
  );
}
