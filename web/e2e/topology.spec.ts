import { appendFileSync, readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { signIn, watchErrors } from "./support";

/*
 * The topology's frame rate (D-149; ARCHITECTURE.md, Performance budgets: 60 fps with 300
 * entities), on a demo of its own sized past 300 devices and clients. The clients view draws every
 * client as its own vertex. A wheel event each frame zooms the map in and out for a few seconds, so
 * the renderer draws every frame; the frames it drew (its `clearRect` calls) per second must reach
 * the budget's minimum in scripts/perf/budgets.json.
 */
const budget = (
  JSON.parse(
    readFileSync(new URL("../../scripts/perf/budgets.json", import.meta.url), "utf8"),
  ) as Record<string, { min: number; unit: string }>
).topology_fps;
const ENTITIES = 300;
const MEASURE_MS = 3_000;

/** The devices and clients a map's label counts ("Network map: 12 devices and 290 clients"). */
function entities(label: string | null): number {
  const counts = /(\d+) devices? and (\d+) clients?/.exec(label ?? "");
  return counts ? Number(counts[1]) + Number(counts[2]) : 0;
}

test("the topology draws 300 entities at 60 fps", async ({ page }) => {
  const errors = watchErrors(page);
  await signIn(page);
  await page.goto("/topology?view=clients");
  const map = page.getByRole("img", { name: /^Network map: / });
  await expect
    .poll(async () => entities(await map.getAttribute("aria-label")), { timeout: 60_000 })
    .toBeGreaterThanOrEqual(ENTITIES);
  // The layout's first positions, and the camera fitted to them.
  await page.waitForTimeout(2_000);

  const fps = await map.evaluate(async (element, ms) => {
    const canvas = element as HTMLCanvasElement;
    const context = canvas.getContext("2d");
    if (!context) throw new Error("no 2D context");
    let draws = 0;
    const clear = context.clearRect;
    context.clearRect = function (...args: Parameters<CanvasRenderingContext2D["clearRect"]>) {
      draws++;
      clear.apply(this, args);
    };
    const box = canvas.getBoundingClientRect();
    const at = { clientX: box.x + box.width / 2, clientY: box.y + box.height / 2 };
    return new Promise<number>((resolve) => {
      let start = 0;
      let frame = 0;
      const step = (now: number) => {
        if (!start) {
          start = now;
          draws = 0;
        }
        frame++;
        // In for 20 frames, out for 20: the camera always moves, and stays within its limits.
        const deltaY = frame % 40 < 20 ? 10 : -10;
        canvas.dispatchEvent(
          new WheelEvent("wheel", { ...at, deltaY, bubbles: true, cancelable: true }),
        );
        if (now - start < ms) {
          requestAnimationFrame(step);
        } else {
          context.clearRect = clear;
          resolve((draws * 1000) / (now - start));
        }
      };
      requestAnimationFrame(step);
    });
  }, MEASURE_MS);

  const verdict = fps >= budget.min ? "ok" : "UNDER BUDGET";
  const line = `Topology frame rate (${ENTITIES}+ entities): ${fps.toFixed(1)} fps (budget ≥ ${budget.min} fps), ${verdict}`;
  process.stdout.write(`${line}\n`);
  if (process.env.GITHUB_STEP_SUMMARY)
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, `- ${line}\n`);
  expect(fps).toBeGreaterThanOrEqual(budget.min);
  expect(errors).toEqual([]);
});
