import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Discovered } from "../../../api/gen/Discovered";
import type { ProbeResult } from "../../../api/gen/ProbeResult";
import { createAppRouter } from "../../../app/routes";
import { adoptJob, summary } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import type { FakeWebSocket } from "../../../test/socket";

const probe: ProbeResult = {
  host: "10.0.0.3",
  port: 22,
  host_key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA test",
  fingerprint: "SHA256:7nq3Wm0dVbxgYQ2hZ0R2pLk0rVqI0n5bT9Vb3c1sXyE",
  algorithm: "ED25519",
};

function discovered(ip: string, hostname: string, mac: string): Discovered {
  return {
    mac,
    ip,
    hostname,
    sources: ["lease", "mesh"],
    seen_by: 1,
    probe: "done",
    ssh: "SSH-2.0-dropbear_2025.88",
    host_key: {
      host_key: `ssh-ed25519 AAAA ${hostname}`,
      fingerprint: `SHA256:${hostname}fingerprint0123456789`,
      algorithm: "ED25519",
    },
    probed_at: 1,
  };
}

type Route = (body: unknown) => unknown;

/** Answers the API by method and path; anything else is the health check. */
function stubApi(routes: Record<string, Route>) {
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    const key = `${init?.method ?? "GET"} ${url}`;
    const route = routes[key];
    if (!route) return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    const answer = route(body);
    return answer instanceof Response ? answer : Response.json(answer);
  });
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

function renderAt(path: string) {
  const { socket, ws } = openTestSocket();
  const history = memoryHistory(path);
  const Router = createAppRouter(history);
  render(() => (
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <SignedInProviders socket={socket}>
        <Router>{(props) => props.children}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "gateway", { role: "gateway" })]);
  ws.receive({ type: "jobs_snapshot", topic: "jobs", jobs: [] });
  return { ws, history };
}

function delta(ws: FakeWebSocket, ...jobs: ReturnType<typeof adoptJob>[]) {
  ws.receive({ type: "jobs_delta", topic: "jobs", jobs });
}

describe("Adoption", () => {
  beforeEach(() => {
    stubApi({});
  });

  it("probes, confirms the fingerprint, follows the job and opens the device", async () => {
    const user = userEvent.setup();
    const posted: unknown[] = [];
    stubApi({
      "POST /api/v1/adopt/probe": (body) => {
        posted.push(body);
        return probe;
      },
      "POST /api/v1/adopt": (body) => {
        posted.push(body);
        return adoptJob(5, "10.0.0.3", 0);
      },
    });
    const { ws, history } = renderAt("/devices");
    await user.click(await screen.findByRole("button", { name: "Adopt device" }));
    const dialog = await screen.findByRole("dialog", { name: "Adopt a device" });
    expect(history.get()).toBe("/devices?adopt=1");
    const address = within(dialog).getByLabelText("Address");
    await waitFor(() => expect(address).toHaveFocus());

    await user.type(address, "a b");
    await user.click(within(dialog).getByRole("button", { name: "Continue" }));
    expect(within(dialog).getByText("Enter an IP address or a host name.")).toBeInTheDocument();
    await user.clear(address);
    await user.type(address, "10.0.0.3");
    await user.type(within(dialog).getByLabelText("Root password"), "secret");
    await user.click(within(dialog).getByLabelText("Keep the password in the vault"));
    await user.click(within(dialog).getByRole("button", { name: "Continue" }));

    expect(await within(dialog).findByTestId("fingerprint")).toHaveTextContent(probe.fingerprint);
    await user.type(within(dialog).getByLabelText("Name"), "attic");
    await user.click(within(dialog).getByLabelText("Access point"));
    await user.click(within(dialog).getByRole("button", { name: "It matches, adopt" }));
    await within(dialog).findByText("Adopting 10.0.0.3");
    expect(posted).toEqual([
      { host: "10.0.0.3" },
      {
        host: "10.0.0.3",
        host_key: probe.host_key,
        password: "secret",
        remember_password: true,
        name: "attic",
        role: "ap",
      },
    ]);
    const steps = () =>
      within(within(dialog).getByRole("list"))
        .getAllByRole("listitem")
        .map((li) => li.textContent);
    expect(steps()[0]).toBe("Log in: In progress");

    delta(ws, adoptJob(5, "10.0.0.3", 5));
    await waitFor(() => expect(steps()[5]).toBe("Back up the configuration: In progress"));
    expect(steps()[1]).toBe("Check the device: Donecheck detail");

    delta(ws, adoptJob(5, "10.0.0.3", 8, "done", { state: "succeeded", node: 7 }));
    await within(dialog).findByText("Adopted. The device is watched in observe mode.");
    await user.click(within(dialog).getByRole("button", { name: "Open the device" }));
    await waitFor(() => expect(history.get()).toBe("/devices/7"));
  });

  it("explains a failure by its code and goes back to the step it concerns", async () => {
    const user = userEvent.setup();
    stubApi({
      "POST /api/v1/adopt/probe": () => probe,
      "POST /api/v1/adopt": () => adoptJob(6, "10.0.0.3", 0),
    });
    const { ws } = renderAt("/devices?adopt=1&host=10.0.0.3");
    const dialog = await screen.findByRole("dialog", { name: "Adopt a device" });
    const address = within(dialog).getByLabelText("Address");
    expect(address).toHaveValue("10.0.0.3");
    await waitFor(() => expect(address).toHaveFocus());
    await user.type(within(dialog).getByLabelText("Root password"), "wrong");
    await user.click(within(dialog).getByRole("button", { name: "Continue" }));
    await user.click(await within(dialog).findByRole("button", { name: "It matches, adopt" }));

    const failed = adoptJob(6, "10.0.0.3", 0, "failed", {
      state: "failed",
      error: { code: "auth_rejected", message: "Cannot log in: root@10.0.0.3:22 rejected it" },
    });
    delta(ws, failed);
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("The device rejected the login");
    expect(alert).toHaveTextContent("Cannot log in: root@10.0.0.3:22 rejected it");
    await user.click(within(dialog).getByRole("button", { name: "Change the password" }));
    expect(within(dialog).getByLabelText("Root password")).toHaveValue("");
    expect(within(dialog).getByLabelText("Address")).toHaveValue("10.0.0.3");
  });

  it("shows why a probe failed", async () => {
    const user = userEvent.setup();
    stubApi({
      "POST /api/v1/adopt/probe": () =>
        new Response(JSON.stringify({ title: "Bad Gateway", detail: "cannot reach 10.0.0.9" }), {
          status: 502,
          headers: { "content-type": "application/problem+json" },
        }),
    });
    renderAt("/devices?adopt=1&host=10.0.0.9");
    const dialog = await screen.findByRole("dialog", { name: "Adopt a device" });
    await user.click(within(dialog).getByRole("button", { name: "Continue" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("cannot reach 10.0.0.9");
    expect(within(dialog).getByRole("button", { name: "Continue" })).toBeEnabled();
  });

  it("lists discovered devices and adopts the selected ones with one password", async () => {
    const user = userEvent.setup();
    const office = discovered("10.0.0.3", "ap-office", "02:00:00:00:00:03");
    const garage = discovered("10.0.0.4", "ap-garage", "02:00:00:00:00:04");
    const mesh: Discovered = {
      ...discovered("", "", "02:00:00:00:00:09"),
      ip: null,
      hostname: null,
      sources: ["mesh"],
      probe: "skipped",
      ssh: null,
      host_key: null,
    };
    let batch: unknown;
    stubApi({
      "GET /api/v1/discovered": () => ({ nodes: [office, garage, mesh], scanning: true }),
      "POST /api/v1/adopt/batch": (body) => {
        batch = body;
        return {
          jobs: [adoptJob(8, "10.0.0.3", 0)],
          refused: [{ host: "10.0.0.4", port: 22, reason: "A job is running for it already" }],
        };
      },
    });
    const { ws } = renderAt("/devices");
    const card = await screen.findByRole("region", { name: "Discovered" });
    expect(within(card).getByText("Scanning")).toBeInTheDocument();
    expect(
      within(card).getAllByText(/Seen by gateway \(DHCP lease, Mesh\)/)[0],
    ).toBeInTheDocument();
    expect(within(card).getByText("No address known")).toBeInTheDocument();
    expect(within(card).getByLabelText("Select 02:00:00:00:00:09")).toBeDisabled();

    expect(within(card).queryByRole("button", { name: /selected/ })).toBeNull();
    await user.click(within(card).getByLabelText("Select ap-office"));
    await user.click(within(card).getByLabelText("Select ap-garage"));
    await user.click(within(card).getByRole("button", { name: "Adopt 2 selected" }));

    const dialog = await screen.findByRole("dialog", { name: "Adopt 2 devices" });
    expect(within(dialog).getByText(office.host_key?.fingerprint ?? "")).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText("Root password"), "secret");
    await user.click(within(dialog).getByRole("button", { name: "Adopt 2 devices" }));
    await within(dialog).findByText("Not started");
    expect(batch).toEqual({
      nodes: [
        { host: "10.0.0.3", host_key: office.host_key?.host_key },
        { host: "10.0.0.4", host_key: garage.host_key?.host_key },
      ],
      password: "secret",
    });
    expect(within(dialog).getByText(": A job is running for it already")).toBeInTheDocument();
    expect(within(dialog).getByRole("progressbar", { name: "ap-office" })).toHaveAttribute(
      "aria-valuenow",
      "6",
    );
    // The card marks the device being adopted.
    expect(within(card).getByText("Adopting")).toBeInTheDocument();

    delta(ws, adoptJob(8, "10.0.0.3", 8, "done", { state: "succeeded", node: 9 }));
    await within(dialog).findByText("1 of 1 adopted");
  });
});
