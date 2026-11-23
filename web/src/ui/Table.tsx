import type { JSX } from "@solidjs/web";
import {
  elementScroll,
  observeElementOffset,
  observeElementRect,
  type VirtualItem,
  Virtualizer,
} from "@tanstack/virtual-core";
import { ArrowDown, ArrowUp, ChevronsUpDown } from "lucide";
import { createEffect, createMemo, createSignal, For, Show, untrack } from "solid-js";
import { Icon } from "./Icon";

export interface Column<T> {
  id: string;
  header: string;
  cell: (row: T) => JSX.Element;
  /** "end" right-aligns the column and sets tabular digits, for numbers. */
  align?: "start" | "end";
  /** Width and other classes for the column's cells, e.g. "w-40". */
  class?: string;
  /** Makes the column sortable from its header: the rows' ascending order. */
  sort?: (a: T, b: T) => number;
}

/** The column the rows are sorted by, and which way. */
export interface SortOrder {
  column: string;
  desc?: boolean;
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
  /** The order at first (a column with `sort`); the rows' own order without one. */
  sortBy?: SortOrder;
  class?: string;
}

const VIRTUALIZE_ABOVE = 200;

/** A data table with a sticky header; long tables render only the rows in view. */
export function Table<T>(props: TableProps<T>) {
  const virtual = () => props.rows.length > (props.virtualizeAbove ?? VIRTUALIZE_ABOVE);
  const [order, setOrder] = createSignal<SortOrder | undefined>(untrack(() => props.sortBy));
  const rows = createMemo(() => {
    const by = order();
    const compare = by && props.columns.find((column) => column.id === by.column)?.sort;
    if (!by || !compare) return props.rows;
    return [...props.rows].sort(by.desc ? (a, b) => compare(b, a) : compare);
  });
  const sorted = (column: Column<T>) => {
    const by = order();
    if (by?.column !== column.id) return "none";
    return by.desc ? "descending" : "ascending";
  };
  const toggle = (column: Column<T>) =>
    setOrder((by) => ({ column: column.id, desc: by?.column === column.id && !by.desc }));
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
                  aria-sort={column.sort ? sorted(column) : undefined}
                  class={[
                    "sticky top-0 z-[1] h-8 whitespace-nowrap border-edge border-b bg-surface px-4 font-medium text-fg-subtle text-xs glass:backdrop-blur-panel",
                    column.align === "end" ? "text-right" : "text-left",
                    column.class,
                  ]}
                >
                  <Show when={column.sort} fallback={column.header}>
                    <button
                      type="button"
                      onClick={() => toggle(column)}
                      class={[
                        "inline-flex items-center gap-1 rounded-control hover:text-fg focus-visible:outline-2 focus-visible:outline-accent",
                        { "flex-row-reverse": column.align === "end" },
                      ]}
                    >
                      {column.header}
                      <Icon
                        icon={
                          sorted(column) === "ascending"
                            ? ArrowUp
                            : sorted(column) === "descending"
                              ? ArrowDown
                              : ChevronsUpDown
                        }
                        class={["size-3", { "opacity-40": sorted(column) === "none" }]}
                      />
                    </button>
                  </Show>
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
          <Show when={virtual()} fallback={<PlainBody table={props} rows={rows()} />}>
            <VirtualBody table={props} rows={rows()} />
          </Show>
        </Show>
      </table>
    </div>
  );
}

/** A body's rows, in the order shown, and the table they belong to. */
interface BodyProps<T> {
  table: TableProps<T>;
  rows: readonly T[];
}

function PlainBody<T>(props: BodyProps<T>) {
  return (
    <tbody>
      <For each={props.rows} keyed={props.table.rowKey}>
        {(row) => <Row table={props.table} row={row()} />}
      </For>
    </tbody>
  );
}

function VirtualBody<T>(props: BodyProps<T>) {
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
      return row === undefined ? index : props.table.rowKey(row);
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
                table={props.table}
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
        "group h-row border-edge border-b last:border-b-0",
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
