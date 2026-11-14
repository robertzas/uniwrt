import { Check, CheckCheck, ScanLine, Trash, Undo2 } from "lucide";
import { For, Show } from "solid-js";
import type { Wall } from "../../../api/gen/Wall";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Icon } from "../../../ui/Icon";
import type { WallsState } from "./createWalls";
import { MATERIALS } from "./materials";

const t = strings.floors.walls;

/**
 * Drawing walls (M6.2c), over the plan's corner: the material new walls get (and the selected
 * wall's or suggestion's, which picking another changes), undo, delete, suggest walls from the
 * plan (accept or delete the one selected, or all of them), done, and what a click does next.
 */
export function WallsToolbar(props: {
  state: WallsState;
  /** A length in plan pixels as the floor shows distances, without a scale undefined. */
  length: (px: number) => string | undefined;
  onDone: () => void;
}) {
  const selected = (): Wall | undefined => {
    const i = props.state.selected();
    return i === undefined ? undefined : props.state.walls()[i];
  };
  const picked = (): Wall | undefined => {
    const i = props.state.picked();
    return i === undefined ? undefined : props.state.suggestions()[i];
  };
  const lengthOf = (wall: Wall) =>
    props.length(Math.hypot(wall.to.x - wall.from.x, wall.to.y - wall.from.y));
  const hint = () => {
    if (props.state.chain()) return t.drawing;
    const suggestion = picked();
    if (suggestion) return t.suggestion(t.materials[suggestion.material], lengthOf(suggestion));
    const wall = selected();
    if (wall) return t.selected(t.materials[wall.material], lengthOf(wall));
    if (props.state.tracing()) return t.tracing;
    if (props.state.nothing()) return t.noneFound;
    if (props.state.suggestions().length > 0) return t.review;
    return t.start;
  };
  return (
    <div
      role="toolbar"
      aria-label={t.toolbar}
      class="absolute top-2 left-2 flex max-w-[calc(100%-4rem)] flex-col gap-2 rounded-control border border-edge bg-surface/95 p-2 shadow-card backdrop-blur"
    >
      <div class="flex flex-wrap items-center gap-1">
        <fieldset class="flex flex-wrap gap-0.5">
          <legend class="sr-only">{t.material}</legend>
          <For each={MATERIALS}>
            {(m) => (
              <button
                type="button"
                aria-pressed={props.state.material() === m.material ? "true" : "false"}
                class={[
                  "inline-flex h-7 items-center gap-1.5 rounded-control px-2 text-xs focus-visible:outline-2 focus-visible:outline-accent",
                  props.state.material() === m.material
                    ? "bg-accent/15 font-medium text-fg"
                    : "text-fg-muted hover:bg-surface-raised hover:text-fg",
                ]}
                onClick={() => props.state.pickMaterial(m.material)}
              >
                <svg width="14" height="8" aria-hidden="true">
                  <path
                    d="M1 4H13"
                    stroke={m.color}
                    stroke-width={Math.min(6, m.width)}
                    stroke-linecap="round"
                  />
                </svg>
                {t.materials[m.material]}
              </button>
            )}
          </For>
        </fieldset>
        <span class="mx-1 h-5 w-px bg-edge" aria-hidden="true" />
        <Button
          size="sm"
          variant="ghost"
          disabled={!props.state.canUndo()}
          onClick={() => props.state.undo()}
        >
          <Icon icon={Undo2} class="size-3.5" />
          {t.undo}
        </Button>
        <Show when={selected()}>
          <Button size="sm" variant="ghost" onClick={() => props.state.remove()}>
            <Icon icon={Trash} class="size-3.5" />
            {t.delete}
          </Button>
        </Show>
        <Show when={picked()}>
          <Button size="sm" variant="secondary" onClick={() => props.state.accept()}>
            <Icon icon={Check} class="size-3.5" />
            {t.accept}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => props.state.dismiss()}>
            <Icon icon={Trash} class="size-3.5" />
            {t.dismiss}
          </Button>
        </Show>
        <Button
          size="sm"
          variant="ghost"
          disabled={props.state.tracing()}
          onClick={() => props.state.suggest()}
        >
          <Icon icon={ScanLine} class="size-3.5" />
          {t.suggest}
        </Button>
        <Button size="sm" variant="primary" onClick={() => props.onDone()}>
          <Icon icon={Check} class="size-3.5" />
          {t.done}
        </Button>
      </div>
      <Show when={props.state.suggestions().length > 0}>
        <div class="flex flex-wrap items-center gap-1">
          <span class="mr-1 text-xs">{t.suggested(props.state.suggestions().length)}</span>
          <Button size="sm" variant="secondary" onClick={() => props.state.acceptAll()}>
            <Icon icon={CheckCheck} class="size-3.5" />
            {t.acceptAll}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => props.state.dismissAll()}>
            <Icon icon={Trash} class="size-3.5" />
            {t.dismissAll}
          </Button>
        </div>
      </Show>
      <p role="status" class="max-w-xl text-fg-muted text-xs">
        {hint()}
      </p>
    </div>
  );
}
