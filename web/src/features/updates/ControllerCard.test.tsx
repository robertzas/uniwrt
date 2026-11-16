import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ControllerUpdate } from "../../api/gen/ControllerUpdate";
import { INSTALL_POLL_MS, installOutcome } from "../../lib/controller";
import { controllerUpdate } from "../../test/controller";
import { ControllerCard } from "./ControllerCard";

/** Answers `v1/controller/update` with what `answers` returns, call by call; records the rest. */
function stubController(answers: (method: string, body: unknown) => Response | ControllerUpdate) {
  const calls: [string, unknown][] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = typeof init?.body === "string" ? JSON.parse(init.body) : undefined;
      calls.push([`${method} ${url}`, body]);
      const answer = answers(`${method} ${url}`, body);
      return answer instanceof Response ? answer : Response.json(answer);
    }),
  );
  return calls;
}

describe("ControllerCard", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("tells how an install ended from a read of the controller", () => {
    const update = controllerUpdate();
    expect(installOutcome("1.2.4", update)).toBeUndefined();
    expect(installOutcome("1.2.4", { ...update, current: "1.2.4" })).toEqual({
      kind: "running",
      version: "1.2.4",
    });
    expect(
      installOutcome("1.2.4", {
        ...update,
        failure: { version: "1.2.4", reason: "it exited", at: 1 },
      }),
    ).toEqual({ kind: "rolled_back", version: "1.2.4", reason: "it exited" });
    expect(
      installOutcome("1.2.4", {
        ...update,
        progress: { state: "failed", version: "1.2.4", reason: "no attestation" },
      }),
    ).toEqual({ kind: "failed", version: "1.2.4", reason: "no attestation" });
  });

  it("installs the release on offer and follows the restart until it runs", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    let reads = 0;
    const calls = stubController((key) => {
      if (key === "POST /api/v1/controller/update") {
        return controllerUpdate({ progress: { state: "downloading", version: "1.2.4" } });
      }
      reads += 1;
      if (reads === 1) return controllerUpdate();
      // The old controller restarting, then gone, then the new one.
      if (reads === 2) {
        return controllerUpdate({ progress: { state: "restarting", version: "1.2.4" } });
      }
      if (reads === 3) return new Response(null, { status: 502 });
      return controllerUpdate({ current: "1.2.4", offer: null });
    });
    render(() => <ControllerCard admin />);
    const card = await screen.findByRole("region", { name: "Controller" });
    expect(within(card).getByText("Running uniwrt 1.2.3")).toBeTruthy();
    expect(within(card).getByText("uniwrt 1.2.4 is available")).toBeTruthy();
    expect(
      within(card).getByRole("link", { name: "Release notes 1.2.4" }).getAttribute("href"),
    ).toContain("v1.2.4");

    await user.click(within(card).getByRole("button", { name: "Install" }));
    expect(calls).toContainEqual(["POST /api/v1/controller/update", { version: "1.2.4" }]);
    expect(await within(card).findByText("Downloading and verifying 1.2.4…")).toBeTruthy();
    expect(within(card).getByRole("button", { name: "Install" })).toBeDisabled();

    await vi.advanceTimersByTimeAsync(INSTALL_POLL_MS);
    expect(await within(card).findByText(/Restarting into 1\.2\.4/)).toBeTruthy();
    await vi.advanceTimersByTimeAsync(INSTALL_POLL_MS * 2);
    expect(
      await within(card).findByText("uniwrt 1.2.4 is running. Reload the page for its interface."),
    ).toBeTruthy();
    expect(within(card).getByText("Running uniwrt 1.2.4")).toBeTruthy();
    const after = reads;
    await vi.advanceTimersByTimeAsync(INSTALL_POLL_MS * 2);
    expect(reads).toBe(after);
  });

  it("shows a rolled-back release, why a controller can't install, and hides Install from viewers", async () => {
    stubController(() =>
      controllerUpdate({
        offer: null,
        blocked: "Demo mode doesn't install releases",
        failure: { version: "1.2.5", reason: "never answered healthy", at: Date.UTC(2026, 10, 16) },
      }),
    );
    render(() => <ControllerCard admin={false} />);
    const card = await screen.findByRole("region", { name: "Controller" });
    expect(within(card).getByText("Demo mode doesn't install releases")).toBeTruthy();
    expect(within(card).getByRole("alert").textContent).toContain("1.2.5 failed its health check");
    expect(within(card).getByText("This controller runs the newest release.")).toBeTruthy();
    expect(within(card).getByText("Automatic install off")).toBeTruthy();
    expect(within(card).queryByRole("button")).toBeNull();
    expect(within(card).queryByRole("checkbox")).toBeNull();
  });

  it("turns the automatic install on and picks its window", async () => {
    const user = userEvent.setup();
    const calls = stubController((key, body) =>
      key.startsWith("PUT") ? (body as ControllerUpdate) : controllerUpdate(),
    );
    render(() => <ControllerCard admin />);
    const card = await screen.findByRole("region", { name: "Controller" });
    await user.click(within(card).getByRole("checkbox", { name: /Install new releases/ }));
    await waitFor(() =>
      expect(calls).toContainEqual([
        "PUT /api/v1/controller/update/settings",
        { auto: true, at_min: 150 },
      ]),
    );
    const select = await within(card).findByRole("combobox", { name: "Window opens at" });
    expect((select as HTMLSelectElement).value).toBe("150");
    await user.selectOptions(select, "120");
    await waitFor(() =>
      expect(calls).toContainEqual([
        "PUT /api/v1/controller/update/settings",
        { auto: true, at_min: 120 },
      ]),
    );
  });

  it("stays hidden when the controller has no updates", async () => {
    const calls = stubController(() => new Response(null, { status: 503 }));
    render(() => <ControllerCard admin />);
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(screen.queryByRole("region", { name: "Controller" })).toBeNull();
  });
});
