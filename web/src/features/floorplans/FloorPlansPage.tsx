import { useSearchParams } from "@solidjs/router";
import { useQuery, useQueryClient } from "@tanstack/solid-query";
import { BrickWall, Map as MapIcon, Pencil, Plus, Ruler, Upload } from "lucide";
import { createEffect, createMemo, createSignal, Show, useContext } from "solid-js";
import type { Band } from "../../api/gen/Band";
import type { Floor } from "../../api/gen/Floor";
import { api } from "../../lib/api";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { EmptyState } from "../../ui/EmptyState";
import { Icon } from "../../ui/Icon";
import { Segmented } from "../../ui/Segmented";
import { CoverageCard } from "./coverage/CoverageCard";
import { COVERAGE_KEY, createCoverage } from "./coverage/createCoverage";
import { CoverageLayer, SuggestedMark } from "./coverage/layers";
import { DevicesPanel } from "./DevicesPanel";
import { FloorDialog } from "./FloorDialog";
import { FloorEditor, type Mode } from "./FloorEditor";
import { withFloor, withoutNode } from "./floors";
import { formatLength } from "./units";
import { createWalls } from "./walls/createWalls";

const t = strings.floors;
const KEY = ["floors"];
const COVERAGE_BANDS: readonly string[] = ["2g", "5g", "6g"];

/**
 * `/insights/floor-plans` (features/wifi.md, Coverage & floor plans; D-117): the site's floors
 * (the one shown is in the URL, `?floor=<id>`), each plan with its devices and scale. Admins add
 * and edit floors, upload plans, set scales and place devices; edits save at once.
 */
export function FloorPlansPage() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const admin = () => session.user.role === "admin";
  const [params, setParams] = useSearchParams<{ floor?: string; coverage?: string }>();
  const client = useQueryClient();
  const query = useQuery(() => ({
    queryKey: KEY,
    queryFn: ({ signal }) => api.floors.list(signal),
  }));
  const floors = (): readonly Floor[] => (query.isSuccess ? query.data : []);
  const update = (change: (list: readonly Floor[]) => Floor[]) =>
    client.setQueryData<Floor[]>(KEY, (old) => change(old ?? []));
  const current = () => floors().find((f) => String(f.id) === params.floor) ?? floors()[0];
  const [mode, setMode] = createSignal<Mode>({ kind: "idle" });
  const [dialog, setDialog] = createSignal<{ open: boolean; floor?: Floor }>({ open: false });

  /** The band whose coverage shows (`?coverage=5g`), if any. */
  const band = () =>
    COVERAGE_BANDS.includes(params.coverage ?? "") ? (params.coverage as Band) : undefined;
  /** Placements and walls feed the model's fit: fetch it again after any change. */
  const refit = () => {
    if (band()) void client.invalidateQueries({ queryKey: COVERAGE_KEY });
  };
  const saved = (floor: Floor) => {
    update((list) => withFloor(list, floor));
    refit();
  };
  const walls = createWalls({ floor: current, onSaved: saved });
  /** The floors with the walls as drawn, ahead of their saves: the heatmap follows each edit. */
  const drawn = createMemo((): readonly Floor[] => {
    const shown = current();
    const list = walls.walls();
    return floors().map((f) =>
      f.id === shown?.id && f.walls !== list ? { ...f, walls: [...list] } : f,
    );
  });
  const coverage = createCoverage({
    floor: () => drawn().find((f) => f.id === current()?.id),
    floors: drawn,
    band,
  });
  // Another floor shown ends whatever the plan was doing.
  createEffect(
    () => current()?.id,
    () => {
      setMode({ kind: "idle" });
    },
  );
  createEffect(
    () => t.title,
    (title) => {
      document.title = `${title} · ${strings.appName}`;
    },
  );

  const summary = (floor: Floor) =>
    [
      t.level(floor.level),
      formatLength(floor.height_m, floor.scale?.unit ?? "m"),
      floor.image && !floor.scale ? t.scale.none : undefined,
    ]
      .filter(Boolean)
      .join(" · ");

  return (
    <div class="mx-auto flex max-w-7xl flex-col gap-4">
      <header class="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 class="font-semibold text-2xl tracking-tight">{t.title}</h1>
          <p class="mt-1 text-fg-muted text-sm">{strings.nav.floorPlans.description}</p>
        </div>
        <Show when={admin() && floors().length > 0}>
          <Button variant="secondary" onClick={() => setDialog({ open: true })}>
            <Icon icon={Plus} class="size-4" />
            {t.add}
          </Button>
        </Show>
      </header>
      <Show
        when={query.isSuccess}
        fallback={
          <p role="status" class="py-20 text-center text-fg-subtle text-sm">
            {query.isError ? t.failed : `${t.loading}…`}
          </p>
        }
      >
        <Show
          when={current()}
          fallback={
            <EmptyState
              icon={MapIcon}
              title={t.empty.title}
              description={admin() ? t.empty.admin : t.empty.viewer}
            >
              <Show when={admin()}>
                <Button class="mt-5" variant="primary" onClick={() => setDialog({ open: true })}>
                  <Icon icon={Plus} class="size-4" />
                  {t.add}
                </Button>
              </Show>
            </EmptyState>
          }
        >
          {(floor) => (
            <>
              <Show when={floors().length > 1}>
                <Segmented
                  label={t.floor}
                  class="self-start"
                  value={String(floor().id)}
                  options={[...floors()]
                    .reverse()
                    .map((f) => ({ value: String(f.id), label: f.name }))}
                  onChange={(id) => setParams({ floor: id })}
                />
              </Show>
              <div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_20rem]">
                <Card aria-label={floor().name}>
                  <CardHeader
                    title={floor().name}
                    description={summary(floor())}
                    actions={
                      <Show when={admin()}>
                        <div class="flex gap-1.5">
                          <Show when={floor().image}>
                            <Button
                              size="sm"
                              variant={mode().kind === "walls" ? "primary" : "secondary"}
                              aria-pressed={mode().kind === "walls" ? "true" : "false"}
                              onClick={() =>
                                setMode(
                                  mode().kind === "walls" ? { kind: "idle" } : { kind: "walls" },
                                )
                              }
                            >
                              <Icon icon={BrickWall} class="size-3.5" />
                              {t.walls.button}
                            </Button>
                            <Button
                              size="sm"
                              variant={mode().kind === "scale" ? "primary" : "secondary"}
                              aria-pressed={mode().kind === "scale" ? "true" : "false"}
                              onClick={() =>
                                setMode(
                                  mode().kind === "scale" ? { kind: "idle" } : { kind: "scale" },
                                )
                              }
                            >
                              <Icon icon={Ruler} class="size-3.5" />
                              {floor().scale ? t.scale.change : t.scale.set}
                            </Button>
                          </Show>
                          <Button
                            size="sm"
                            variant="ghost"
                            onClick={() => setDialog({ open: true, floor: floor() })}
                          >
                            <Icon icon={Pencil} class="size-3.5" />
                            {t.edit}
                          </Button>
                        </div>
                      </Show>
                    }
                  />
                  <div class="px-4 pb-4">
                    <Show
                      when={floor().image}
                      fallback={
                        <EmptyState
                          icon={Upload}
                          title={t.noPlan.title}
                          description={admin() ? t.noPlan.admin : t.noPlan.viewer}
                        >
                          <Show when={admin()}>
                            <Button
                              class="mt-5"
                              variant="primary"
                              onClick={() => setDialog({ open: true, floor: floor() })}
                            >
                              <Icon icon={Upload} class="size-4" />
                              {t.noPlan.upload}
                            </Button>
                          </Show>
                        </EmptyState>
                      }
                    >
                      <FloorEditor
                        floor={floor()}
                        nodes={live.fleet.nodes}
                        admin={admin()}
                        mode={mode()}
                        onMode={setMode}
                        onSaved={saved}
                        walls={walls}
                        underlay={(moving) => (
                          <Show when={coverage.heat(moving)}>
                            {(grid) => <CoverageLayer grid={grid()} />}
                          </Show>
                        )}
                        marks={(frame) => (
                          <SuggestedMark
                            frame={frame}
                            suggestion={coverage.summary()?.suggestion}
                          />
                        )}
                      />
                    </Show>
                  </div>
                </Card>
                <div class="flex flex-col gap-4">
                  <CoverageCard
                    floor={floor()}
                    band={band()}
                    onBand={(b) => setParams({ coverage: b })}
                    state={coverage}
                  />
                  <DevicesPanel
                    floor={floor()}
                    floors={floors()}
                    nodes={live.fleet.nodes}
                    admin={admin()}
                    mode={mode()}
                    onMode={setMode}
                    onRemoved={(node) => {
                      update((list) => withoutNode(list, floor().id, node));
                      refit();
                    }}
                  />
                </div>
              </div>
            </>
          )}
        </Show>
      </Show>
      <FloorDialog
        open={dialog().open}
        onOpenChange={(open) => setDialog((d) => ({ ...d, open }))}
        floor={dialog().floor}
        floors={floors()}
        onSaved={(floor) => {
          saved(floor);
          setParams({ floor: String(floor.id) });
        }}
        onDeleted={(id) => {
          update((list) => list.filter((f) => f.id !== id));
          setParams({ floor: undefined });
        }}
      />
    </div>
  );
}
