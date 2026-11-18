import { render, screen, waitFor, within } from "@solidjs/testing-library";
import type { JSX } from "@solidjs/web";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ControllerBackupStatus } from "../../../api/gen/ControllerBackupStatus";
import type { RestoreState } from "../../../api/gen/RestoreState";
import { followRestore, INSTALL_POLL_MS, RESTART_LIMIT_MS } from "../../../lib/controller";
import { formatDateTime } from "../../../lib/format";
import { BackupsTab } from "./BackupsTab";
import { SetupRestore } from "./ControllerRestore";

const T0 = Date.UTC(2026, 10, 18, 2, 0);
const MANIFEST = { format: 1, version: "1.4.0", schema: 42, taken_at: T0 };

let calls: { method: string; url: string; body: unknown }[] = [];
let status: ControllerBackupStatus;
let restore: RestoreState;
let backUpAnswer: () => Response;

function statusOf(extra: Partial<ControllerBackupStatus> = {}): ControllerBackupStatus {
  return {
    passphrase_set: true,
    dir: "/data/backups",
    extra_dir: "/mnt/nas",
    backups: [
      {
        name: "uniwrt-backup-20261118T020000Z.tar.gz.age",
        taken_at: T0,
        size_bytes: 3_200_000,
        copied: true,
      },
      {
        name: "uniwrt-backup-20261117T020000Z.tar.gz.age",
        taken_at: T0 - 86_400_000,
        size_bytes: 3_100_000,
        copied: false,
      },
    ],
    running: false,
    last: { at: T0, error: null },
    restore: { state: "none" },
    at_min: 120,
    ...extra,
  };
}

beforeEach(() => {
  calls = [];
  status = statusOf();
  restore = { state: "none" };
  backUpAnswer = () => Response.json(status.backups[0], { status: 201 });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = typeof init?.body === "string" ? JSON.parse(init.body) : init?.body;
      calls.push({ method, url, body });
      const key = `${method} ${url}`;
      if (key === "GET /api/v1/controller/backups") return Response.json(status);
      if (key === "POST /api/v1/controller/backups") return backUpAnswer();
      if (key === "PUT /api/v1/controller/backups/passphrase") {
        status = { ...status, passphrase_set: true };
        return new Response(null, { status: 204 });
      }
      if (key === "GET /api/v1/controller/restore") return Response.json(restore);
      if (key === "PUT /api/v1/controller/restore") {
        restore = { state: "uploaded", size_bytes: 3_200_000 };
        return Response.json(restore, { status: 201 });
      }
      if (key === "POST /api/v1/controller/restore/unlock") {
        if ((body as { passphrase: string }).passphrase !== "correct horse battery") {
          return Response.json(
            { title: "Bad Request", detail: "The passphrase doesn't open this backup" },
            { status: 400, headers: { "content-type": "application/problem+json" } },
          );
        }
        restore = { state: "ready", manifest: MANIFEST };
        return Response.json(restore);
      }
      if (key === "POST /api/v1/controller/restore/apply")
        return new Response(null, { status: 202 });
      if (key === "DELETE /api/v1/controller/restore") {
        restore = { state: "none" };
        return new Response(null, { status: 204 });
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function withQueries(ui: () => JSX.Element) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(() => <QueryClientProvider client={client}>{ui()}</QueryClientProvider>);
}

describe("BackupsTab", () => {
  it("lists the backups, where they are kept and how the last went", async () => {
    status = statusOf({ last: { at: T0, error: "disk full" } });
    withQueries(() => <BackupsTab admin />);
    const card = await screen.findByRole("region", { name: "Controller backups" });
    expect(within(card).getByText(/every night at 02:00/i)).toBeInTheDocument();
    expect(within(card).getByRole("alert")).toHaveTextContent(
      `The backup at ${formatDateTime(T0)} failed: disk full`,
    );
    const rows = within(card).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent(`Taken ${formatDateTime(T0)}`);
    expect(within(rows[0] as HTMLElement).getByText("Copied")).toBeInTheDocument();
    expect(within(rows[1] as HTMLElement).getByText("Not copied")).toBeInTheDocument();
    expect(
      within(rows[0] as HTMLElement).getByRole("link", { name: /Download the backup/ }),
    ).toHaveAttribute(
      "href",
      "/api/v1/controller/backups/uniwrt-backup-20261118T020000Z.tar.gz.age",
    );
    expect(card).toHaveTextContent("Kept in /data/backups and copied to /mnt/nas.");
  });

  it("warns while no passphrase is set and keeps viewers to reading", async () => {
    status = statusOf({ passphrase_set: false, backups: [], last: null, extra_dir: null });
    withQueries(() => <BackupsTab admin={false} />);
    const card = await screen.findByRole("region", { name: "Controller backups" });
    expect(within(card).getByRole("alert")).toHaveTextContent(
      "No backups are taken until an admin sets a passphrase.",
    );
    expect(within(card).getByText("No backups yet.")).toBeInTheDocument();
    expect(card).toHaveTextContent("--backup-dir");
    expect(screen.queryByRole("button", { name: "Back up now" })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Passphrase" })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Restore" })).not.toBeInTheDocument();
  });

  it("sets the passphrase once it is long enough and confirmed", async () => {
    status = statusOf({ passphrase_set: false });
    const user = userEvent.setup();
    withQueries(() => <BackupsTab admin />);
    expect(await screen.findByRole("button", { name: "Back up now" })).toBeDisabled();
    const card = screen.getByRole("region", { name: "Passphrase" });
    await user.type(within(card).getByLabelText("New passphrase"), "short");
    await user.type(within(card).getByLabelText("Confirm passphrase"), "other");
    await user.click(within(card).getByRole("button", { name: "Set passphrase" }));
    expect(within(card).getByText("The passphrases don’t match.")).toBeInTheDocument();
    expect(calls.some((call) => call.url.endsWith("/passphrase"))).toBe(false);

    await user.clear(within(card).getByLabelText("New passphrase"));
    await user.clear(within(card).getByLabelText("Confirm passphrase"));
    await user.type(within(card).getByLabelText("New passphrase"), "correct horse battery");
    await user.type(within(card).getByLabelText("Confirm passphrase"), "correct horse battery");
    await user.click(within(card).getByRole("button", { name: "Set passphrase" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "PUT",
        url: "/api/v1/controller/backups/passphrase",
        body: { passphrase: "correct horse battery" },
      }),
    );
    expect(await screen.findByRole("button", { name: "Back up now" })).toBeEnabled();
    expect(within(card).getByRole("button", { name: "Change passphrase" })).toBeInTheDocument();
  });

  it("backs up now and reads the list again", async () => {
    const user = userEvent.setup();
    withQueries(() => <BackupsTab admin />);
    await user.click(await screen.findByRole("button", { name: "Back up now" }));
    await waitFor(() =>
      expect(calls.filter((call) => call.url === "/api/v1/controller/backups")).toHaveLength(3),
    );
    expect(calls[1]?.method).toBe("POST");
  });

  it("restores from an upload: unlock, what it holds, then the restart", async () => {
    const user = userEvent.setup();
    withQueries(() => <BackupsTab admin />);
    await user.click(await screen.findByRole("button", { name: "Restore from a file…" }));
    const dialog = await screen.findByRole("dialog", { name: "Restore a controller backup" });
    const upload = within(dialog).getByRole("button", { name: "Upload" });
    expect(upload).toBeDisabled();
    const file = new File(["age-encryption.org/v1"], "uniwrt-backup.tar.gz.age");
    await user.upload(within(dialog).getByLabelText("Backup file"), file);
    await user.click(upload);
    const put = await waitFor(() => {
      const found = calls.find((call) => call.method === "PUT" && call.url.endsWith("/restore"));
      expect(found).toBeDefined();
      return found;
    });
    expect(put?.body).toBeInstanceOf(Blob);

    const field = await within(dialog).findByLabelText("Backup passphrase");
    await user.type(field, "wrong passphrase");
    await user.click(within(dialog).getByRole("button", { name: "Open" }));
    expect(
      await within(dialog).findByText("The passphrase doesn't open this backup"),
    ).toBeVisible();
    await user.clear(field);
    await user.type(field, "correct horse battery");
    await user.click(within(dialog).getByRole("button", { name: "Open" }));
    expect(await within(dialog).findByText("1.4.0")).toBeInTheDocument();
    expect(dialog).toHaveTextContent(formatDateTime(T0));
    expect(dialog).toHaveTextContent(/Everything on this controller is replaced/);

    await user.click(within(dialog).getByRole("button", { name: "Restore and restart" }));
    expect(await within(dialog).findByText("Restarting with the backup…")).toBeInTheDocument();
    expect(calls.some((call) => call.url === "/api/v1/controller/restore/apply")).toBe(true);
  });

  it("opens a staged restore at its step, and discards it", async () => {
    restore = { state: "ready", manifest: MANIFEST };
    status = statusOf({ restore });
    const user = userEvent.setup();
    withQueries(() => <BackupsTab admin />);
    const card = await screen.findByRole("region", { name: "Restore" });
    expect(card).toHaveTextContent("A backup is waiting to be restored.");
    await user.click(within(card).getByRole("button", { name: "Continue" }));
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText("1.4.0")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Discard" }));
    expect(await within(dialog).findByLabelText("Backup file")).toBeInTheDocument();
    expect(calls.some((call) => call.method === "DELETE")).toBe(true);
  });
});

describe("SetupRestore", () => {
  it("opens the restore from the first-run screen, saying what the new host takes on", async () => {
    restore = { state: "ready", manifest: MANIFEST };
    const user = userEvent.setup();
    render(() => <SetupRestore />);
    await user.click(screen.getByRole("button", { name: /Restore a backup instead/ }));
    const dialog = await screen.findByRole("dialog", { name: "Restore a backup" });
    expect(await within(dialog).findByText(/takes on the backup’s network/)).toBeInTheDocument();
  });
});

describe("followRestore", () => {
  function follow(health: () => Promise<unknown>, state: () => Promise<RestoreState>) {
    const onBack = vi.fn();
    const onLost = vi.fn();
    const stop = followRestore({ health, state, onBack, onLost });
    return { onBack, onLost, stop };
  }

  it("is back once the controller answers after being away", async () => {
    vi.useFakeTimers();
    let up = false;
    const { onBack } = follow(
      () => (up ? Promise.resolve({}) : Promise.reject(new Error("away"))),
      () => Promise.resolve({ state: "ready", manifest: MANIFEST }),
    );
    await vi.advanceTimersByTimeAsync(INSTALL_POLL_MS * 3);
    expect(onBack).not.toHaveBeenCalled();
    up = true;
    await vi.advanceTimersByTimeAsync(INSTALL_POLL_MS);
    expect(onBack).toHaveBeenCalledOnce();
  });

  it("is back when the restore is no longer staged, or its state is refused", async () => {
    vi.useFakeTimers();
    let state: Promise<RestoreState> = Promise.resolve({ state: "ready", manifest: MANIFEST });
    const { onBack } = follow(
      () => Promise.resolve({}),
      () => state,
    );
    await vi.advanceTimersByTimeAsync(INSTALL_POLL_MS * 2);
    expect(onBack).not.toHaveBeenCalled();
    state = Promise.resolve({ state: "none" });
    await vi.advanceTimersByTimeAsync(INSTALL_POLL_MS);
    expect(onBack).toHaveBeenCalledOnce();

    const refused = follow(
      () => Promise.resolve({}),
      () => Promise.reject(new Error("401")),
    );
    await vi.advanceTimersByTimeAsync(INSTALL_POLL_MS);
    expect(refused.onBack).toHaveBeenCalledOnce();
  });

  it("gives up after the restart limit", async () => {
    vi.useFakeTimers();
    const { onBack, onLost } = follow(
      () => Promise.reject(new Error("away")),
      () => Promise.resolve({ state: "none" }),
    );
    await vi.advanceTimersByTimeAsync(RESTART_LIMIT_MS + INSTALL_POLL_MS * 2);
    expect(onLost).toHaveBeenCalledOnce();
    expect(onBack).not.toHaveBeenCalled();
  });
});
