import { memoryHistory } from "@solidjs/router";
import { fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Coverage } from "../../api/gen/Coverage";
import type { Floor } from "../../api/gen/Floor";
import { createAppRouter } from "../../app/routes";
import { summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";

vi.mock("./upload", async (original) => ({
  ...(await original<typeof import("./upload")>()),
  normalizePlan: vi.fn(async () => new Blob(["plan"], { type: "image/webp" })),
}));

let calls: { method: string; url: string; body: unknown }[] = [];
let floors: Floor[] = [];
/** One 5 GHz model, not calibrated: a device alone on the floor. */
const coverage: Coverage = {
  bands: [
    {
      band: "5g",
      loss_at_1m_db: 47.3,
      exponent: 3.3,
      exponent_fitted: false,
      floor_loss_db: 16,
      floor_loss_fitted: false,
      samples: 0,
      error_db: null,
      transmitters: [
        { node: 1, tx_power_dbm: 20 },
        { node: 2, tx_power_dbm: 20 },
      ],
    },
  ],
};

function floor(extra: Partial<Floor> = {}): Floor {
  return {
    id: 1,
    name: "Ground",
    level: 0,
    height_m: 3,
    image: { kind: "webp", width_px: 800, height_px: 600, bytes: 4, version: 1 },
    scale: null,
    nodes: [],
    updated_at: 0,
    ...extra,
  };
}

/** A fake of the floors API: it keeps `floors` and answers as the controller does. */
beforeEach(() => {
  calls = [];
  floors = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = typeof init?.body === "string" ? JSON.parse(init.body) : init?.body;
      calls.push({ method, url, body });
      const path = url.replace("/api/v1", "");
      const one = path.match(/^\/floors\/(\d+)/);
      const found = one ? floors.find((f) => f.id === Number(one[1])) : undefined;
      if (path === "/floors" && method === "GET") return Response.json(floors);
      if (path === "/floors/coverage") return Response.json(coverage);
      if (path === "/floors" && method === "POST") {
        const created = floor({ ...(body as Partial<Floor>), id: 7, image: null });
        floors.push(created);
        return Response.json(created, { status: 201 });
      }
      if (found && /\/image$/.test(path) && method === "PUT") {
        found.image = { kind: "webp", width_px: 800, height_px: 600, bytes: 4, version: 1 };
        return Response.json(found);
      }
      const place = path.match(/\/nodes\/(\d+)$/);
      if (found && place && method === "PUT") {
        const at = body as { x: number; y: number };
        found.nodes = [
          ...found.nodes.filter((p) => p.node !== Number(place[1])),
          { node: Number(place[1]), at },
        ];
        return Response.json(found);
      }
      if (found && method === "PUT") {
        Object.assign(found, body);
        return Response.json(found);
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderPage(role?: "viewer") {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/insights/floor-plans"));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router />
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "Gateway", { role: "gateway" }), summary(2, "Attic AP")]);
}

/** A click on the plan at a point of the canvas (jsdom lays nothing out: the plan sits at 0.95×). */
function clickPlan(x: number, y: number) {
  const plan = screen.getByRole("img", { name: /^Plan of/ });
  const at = { clientX: x, clientY: y, button: 0, pointerId: 1 };
  fireEvent.pointerDown(plan, at);
  fireEvent.pointerUp(plan, at);
}

const sent = (method: string, pattern: RegExp) =>
  calls.filter((c) => c.method === method && pattern.test(c.url));

describe("the floor plans page", () => {
  it("adds the first floor with its plan", async () => {
    await renderPage();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Add floor" }));
    const dialog = await screen.findByRole("dialog");
    await user.type(within(dialog).getByLabelText("Name"), "Ground");
    const file = new File(["png"], "ground.png", { type: "image/png" });
    await user.upload(within(dialog).getByLabelText("Plan"), file);
    await user.click(within(dialog).getByRole("button", { name: "Add floor" }));

    await waitFor(() => expect(sent("PUT", /\/floors\/7\/image$/)).toHaveLength(1));
    expect(sent("POST", /\/floors$/)[0]?.body).toEqual({
      name: "Ground",
      level: 0,
      height_m: 3,
      scale: null,
    });
    expect(await screen.findByRole("img", { name: "Plan of Ground" })).toBeInTheDocument();
    const devices = screen.getByRole("region", { name: "Devices" });
    expect(within(devices).getByRole("button", { name: "Place Attic AP" })).toBeEnabled();
  });

  it("places a device where the plan is clicked, and nudges it with the keys", async () => {
    floors = [floor()];
    await renderPage();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Place Attic AP" }));
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("Click the plan where Attic AP is"),
    );
    // The plan is fitted at 0.95× and centered in its own size: (20, 15) is its corner.
    clickPlan(20 + 95, 15 + 190);
    await waitFor(() => expect(sent("PUT", /\/floors\/1\/nodes\/2$/)).toHaveLength(1));
    const at = sent("PUT", /\/nodes\/2$/)[0]?.body as { x: number; y: number };
    expect(at.x).toBeCloseTo(100);
    expect(at.y).toBeCloseTo(200);

    const marker = await screen.findByRole("button", { name: /^Attic AP: drag/ });
    marker.focus();
    await user.keyboard("{ArrowRight}");
    await waitFor(() => expect(sent("PUT", /\/nodes\/2$/)).toHaveLength(2), { timeout: 2000 });
    const nudged = sent("PUT", /\/nodes\/2$/)[1]?.body as { x: number };
    expect(nudged.x).toBeGreaterThan(105);
  });

  it("sets the scale from two clicks and a length in feet", async () => {
    floors = [floor()];
    await renderPage();
    const user = userEvent.setup();
    await user.click(await screen.findByRole("button", { name: "Set scale" }));
    clickPlan(20, 15);
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("Now click its other end"),
    );
    clickPlan(20 + 380, 15);
    const form = await screen.findByRole("form", { name: "Set scale" });
    await user.type(within(form).getByLabelText("Its real length"), "10");
    await user.selectOptions(within(form).getByLabelText("Unit"), "ft");
    await user.click(within(form).getByRole("button", { name: "Save scale" }));

    await waitFor(() => expect(sent("PUT", /\/floors\/1$/)).toHaveLength(1));
    const body = sent("PUT", /\/floors\/1$/)[0]?.body as Floor;
    expect(body.scale?.unit).toBe("ft");
    expect(body.scale?.length_m).toBeCloseTo(3.048);
    expect(body.scale?.to.x).toBeCloseTo(400);
    expect(await screen.findByRole("img", { name: /^Scale: / })).toBeInTheDocument();
  });

  it("shows viewers the plans without anything to edit", async () => {
    floors = [floor({ nodes: [{ node: 2, at: { x: 10, y: 10 } }] })];
    await renderPage("viewer");
    expect(await screen.findByRole("img", { name: "Plan of Ground" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Attic AP" })).toBeInTheDocument();
    for (const name of ["Add floor", "Edit floor", "Set scale", "Place Gateway", "Take off"]) {
      expect(screen.queryByRole("button", { name })).toBeNull();
    }
  });

  it("shows the coverage on a band: dead zones and where another node would help", async () => {
    // 80 × 60 m with one device near a corner: most of the floor is out of its reach.
    floors = [
      floor({
        scale: { from: { x: 0, y: 0 }, to: { x: 100, y: 0 }, length_m: 10, unit: "m" },
        nodes: [{ node: 2, at: { x: 100, y: 100 } }],
      }),
    ];
    await renderPage();
    const user = userEvent.setup();
    const card = await screen.findByRole("region", { name: "Coverage" });
    expect(sent("GET", /\/floors\/coverage$/)).toHaveLength(0);
    await user.click(within(card).getByRole("button", { name: "5 GHz" }));

    await waitFor(() => expect(card).toHaveTextContent(/Dead zones: \d+ m², \d+ % of the floor/));
    expect(card).toHaveTextContent("Another node at the marked spot would cover about");
    expect(card).toHaveTextContent("Not calibrated yet");
    expect(card).toHaveTextContent("1 device on this band isn't placed");
    expect(card).toHaveTextContent("the whole plan counts");
    expect(document.querySelector("[data-suggested]")).not.toBeNull();
    expect(within(card).getByRole("button", { name: "5 GHz" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });
});
