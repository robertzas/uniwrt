import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { TrustCa } from "./TrustCa";

describe("signed out: trusting the controller's certificate", () => {
  it("reads the fingerprint only once asked, and offers the CA", async () => {
    const fetch = vi.fn(async () =>
      Response.json({
        ca_name: "uniwrt local CA",
        ca_sha256: "AB:CD:EF:01",
        ca_not_after: "2036-11-27T00:00:00Z",
        names: ["localhost"],
        not_after: "2027-02-25T00:00:00Z",
      }),
    );
    vi.stubGlobal("fetch", fetch);
    const user = userEvent.setup();
    render(() => <TrustCa />);
    const toggle = screen.getByRole("button", { name: "Trust this controller's certificate" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(fetch).not.toHaveBeenCalled();
    await user.click(toggle);
    expect(await screen.findByText("AB:CD:EF:01")).toBeVisible();
    expect(fetch.mock.calls.map((call) => String((call as unknown[])[0]))).toEqual(["/api/v1/tls"]);
    expect(screen.getByRole("link", { name: "Download the CA" })).toHaveAttribute(
      "href",
      "/api/v1/tls/ca.crt",
    );
  });

  it("says so when the fingerprint can't be read", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(null, { status: 500 })),
    );
    const user = userEvent.setup();
    render(() => <TrustCa />);
    await user.click(screen.getByRole("button", { name: "Trust this controller's certificate" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be read");
  });
});
