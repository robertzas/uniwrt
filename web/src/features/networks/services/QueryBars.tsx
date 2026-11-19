import { createMemo, createSignal, For, Show } from "solid-js";
import type { DnsRange } from "../../../api/gen/DnsRange";
import { formatCount } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { type Bar, peakOf } from "./dnsStats";

const t = strings.networks.queries;
const hourLabel = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });
/** The range's ends over 24 h: they are the same hour a day apart. */
const edgeLabel = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  hour: "2-digit",
  minute: "2-digit",
});
const dayLabel = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  day: "numeric",
  month: "short",
});
const ANSWERED = "color-mix(in oklab, var(--color-accent) 70%, transparent)";
const BLOCKED = "var(--color-crit)";
const HOVER = "color-mix(in oklab, var(--color-fg-subtle) 15%, transparent)";

/**
 * The queries over the range as stacked bars, blocked ones on the axis: one bar per hour or per
 * day. Hovering a bar names its span and counts; the start and end of the range sit below.
 */
export function QueryBars(props: { bars: readonly Bar[]; range: DnsRange; label: string }) {
  const [hover, setHover] = createSignal<number>();
  const peak = createMemo(() => peakOf(props.bars));
  const when = (bar: Bar) =>
    (props.range === "24h" ? hourLabel : dayLabel).format(new Date(bar.start));
  const edge = (bar: Bar) =>
    (props.range === "24h" ? edgeLabel : dayLabel).format(new Date(bar.start));
  const height = (count: number) => (100 * count) / peak();
  const shown = () => {
    const index = hover();
    return index === undefined ? undefined : props.bars[index];
  };
  const pick = (event: PointerEvent & { currentTarget: SVGSVGElement }) => {
    const box = event.currentTarget.getBoundingClientRect();
    const count = props.bars.length;
    if (box.width <= 0 || count === 0) return;
    const index = Math.floor(((event.clientX - box.left) / box.width) * count);
    setHover(Math.max(0, Math.min(count - 1, index)));
  };

  return (
    <figure class="flex flex-col gap-1">
      <figcaption class="flex h-5 items-center gap-4 text-fg-subtle text-xs tabular-nums">
        <Show
          when={shown()}
          fallback={
            <>
              <Legend color="bg-accent/70" text={t.answered} />
              <Legend color="bg-crit" text={t.blocked} />
            </>
          }
        >
          {(bar) => (
            <span class="text-fg">
              {t.bar(when(bar()), formatCount(bar().queries), formatCount(bar().blocked))}
            </span>
          )}
        </Show>
      </figcaption>
      <svg
        role="img"
        aria-label={props.label}
        viewBox={`0 0 ${Math.max(1, props.bars.length)} 100`}
        preserveAspectRatio="none"
        class="h-28 w-full touch-none"
        onPointerMove={pick}
        onPointerDown={pick}
        onPointerLeave={() => setHover(undefined)}
      >
        <For each={props.bars}>
          {(bar, index) => {
            const blocked = () => height(bar.blocked);
            const answered = () => height(bar.queries - bar.blocked);
            return (
              <g>
                <Show when={hover() === index()}>
                  <rect x={index()} y={0} width={1} height={100} fill={HOVER} />
                </Show>
                <rect
                  x={index() + 0.12}
                  y={100 - blocked() - answered()}
                  width={0.76}
                  height={answered()}
                  fill={ANSWERED}
                />
                <rect
                  x={index() + 0.12}
                  y={100 - blocked()}
                  width={0.76}
                  height={blocked()}
                  fill={BLOCKED}
                />
              </g>
            );
          }}
        </For>
      </svg>
      <div aria-hidden="true" class="flex justify-between text-fg-subtle text-xs tabular-nums">
        <Show when={props.bars[0]}>{(first) => <span>{edge(first())}</span>}</Show>
        <Show when={props.bars.at(-1)}>{(last) => <span>{edge(last())}</span>}</Show>
      </div>
    </figure>
  );
}

function Legend(props: { color: string; text: string }) {
  return (
    <span class="inline-flex items-center gap-1.5">
      <span aria-hidden="true" class={["inline-block size-2 rounded-sm", props.color]} />
      {props.text}
    </span>
  );
}
