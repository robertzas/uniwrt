import { Check, Trash, Undo2 } from "lucide";
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
 * wall's, which picking another changes), undo, delete, done, and what a click does next.
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
  const hint = () => {
    if (props.state.chain()) return t.drawing;
    const wall = selected();
    if (wall) {
      const px = Math.hypot(wall.to.x - wall.from.x, wall.to.y - wall.from.y);
      return t.selected(t.materials[wall.material], props.length(px));
    }
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
        <Button size="sm" variant="primary" onClick={() => props.onDone()}>
          <Icon icon={Check} class="size-3.5" />
          {t.done}
        </Button>
      </div>
      <p role="status" class="max-w-xl text-fg-muted text-xs">
        {hint()}
      </p>
    </div>
  );
}
