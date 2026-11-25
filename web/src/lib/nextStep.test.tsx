import { cleanup, render, screen } from "@solidjs/testing-library";
import { flush } from "solid-js";
import { describe, expect, it } from "vitest";
import { summary } from "../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../test/providers";
import { type NextStep, SET_UP_WIFI, useNextSteps } from "./nextStep";

function Probe(props: { steps: (next: ReturnType<typeof useNextSteps>) => NextStep[] }) {
  const next = useNextSteps();
  return (
    <output>
      {props
        .steps(next)
        .map((step) => `${step.label} → ${step.href}`)
        .join(" | ")}
    </output>
  );
}

function renderProbe(
  steps: (next: ReturnType<typeof useNextSteps>) => NextStep[],
  role?: "viewer",
  nodes?: ReturnType<typeof summary>[],
) {
  const { socket, ws } = openTestSocket();
  render(() => (
    <SignedInProviders socket={socket} role={role}>
      <Probe steps={steps} />
    </SignedInProviders>
  ));
  if (nodes) sendFleet(ws, nodes);
  flush();
  return screen.getByRole("status");
}

describe("the next steps of empty pages", () => {
  it("adopts first while no device is adopted, then takes the page's own step", () => {
    expect(renderProbe((next) => next.adoptFirst(SET_UP_WIFI), undefined, [])).toHaveTextContent(
      "Adopt a device → /devices?adopt=1",
    );
  });

  it("offers the page's own step, or none, once a device is adopted", () => {
    const nodes = [summary(1, "Gateway", { role: "gateway" })];
    expect(renderProbe((next) => next.adoptFirst(SET_UP_WIFI), undefined, nodes)).toHaveTextContent(
      "Set up Wi-Fi → /wifi",
    );
  });

  it("offers nothing before the fleet is read", () => {
    expect(renderProbe((next) => next.adoptFirst())).toHaveTextContent(/^$/);
  });

  it("offers a page without a gateway its adoption or a new router", () => {
    expect(renderProbe((next) => next.gateway(), undefined, [])).toHaveTextContent(
      "Adopt your gateway → /devices?adopt=1 | Set up a new router → /setup",
    );
  });

  it("offers a viewer nothing", () => {
    expect(renderProbe((next) => next.adoptFirst(), "viewer", [])).toHaveTextContent(/^$/);
    cleanup();
    expect(renderProbe((next) => next.gateway(), "viewer", [])).toBeEmptyDOMElement();
  });
});
