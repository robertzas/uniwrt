import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SupportTab } from "./SupportTab";

let calls: string[] = [];
let answer: () => Response;
const saved: { href: string; download: string }[] = [];

beforeEach(() => {
  calls = [];
  saved.length = 0;
  answer = () =>
    new Response(new Blob(["gz"]), {
      headers: {
        "content-type": "application/gzip",
        "content-disposition": 'attachment; filename="uniwrt-support-20261124-0402.tar.gz"',
      },
    });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      calls.push(url);
      return answer();
    }),
  );
  vi.stubGlobal(
    "URL",
    Object.assign(URL, { createObjectURL: () => "blob:x", revokeObjectURL: () => {} }),
  );
  vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (
    this: HTMLAnchorElement,
  ) {
    saved.push({ href: this.href, download: this.download });
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Settings → Support", () => {
  it("downloads the bundle for an admin", async () => {
    const user = userEvent.setup();
    render(() => <SupportTab admin />);
    expect(screen.getByText(/look it over before sharing/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Download the bundle" }));
    await waitFor(() => expect(saved).toHaveLength(1));
    expect(calls).toEqual(["/api/v1/support/bundle"]);
    expect(saved[0]?.download).toBe("uniwrt-support-20261124-0402.tar.gz");
  });

  it("says why a download failed", async () => {
    const user = userEvent.setup();
    answer = () =>
      new Response(
        JSON.stringify({ title: "Forbidden", detail: "Only admins download the support bundle" }),
        {
          status: 403,
          headers: { "content-type": "application/problem+json" },
        },
      );
    render(() => <SupportTab admin />);
    await user.click(screen.getByRole("button", { name: "Download the bundle" }));
    expect(await screen.findByText(/Only admins download the support bundle/)).toBeInTheDocument();
    expect(saved).toHaveLength(0);
  });

  it("offers no download to others", () => {
    render(() => <SupportTab admin={false} />);
    expect(screen.queryByRole("button", { name: "Download the bundle" })).not.toBeInTheDocument();
    expect(screen.getByText("Only admins can download the support bundle.")).toBeInTheDocument();
  });
});
