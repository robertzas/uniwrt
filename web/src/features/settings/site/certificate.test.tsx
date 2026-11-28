import { render, screen, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TlsInfo } from "../../../api/gen/TlsInfo";
import { CertificateCard } from "./CertificateCard";

const info: TlsInfo = {
  ca_name: "uniwrt local CA 1a2b",
  ca_sha256: "AB:CD:EF:01",
  ca_not_after: "2036-11-27T00:00:00Z",
  names: ["localhost", "uniwrt.lan", "192.168.1.10"],
  not_after: "2027-02-25T00:00:00Z",
};

beforeEach(() => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) =>
      url === "/api/v1/tls" ? Response.json(info) : new Response(null, { status: 404 }),
    ),
  );
});

describe("Settings › Site › HTTPS certificate", () => {
  it("offers the CA with its fingerprint, the names covered and the expiry", async () => {
    render(() => (
      <QueryClientProvider client={new QueryClient()}>
        <CertificateCard />
      </QueryClientProvider>
    ));
    const card = await screen.findByRole("region", { name: "HTTPS certificate" });
    expect(await within(card).findByText("AB:CD:EF:01")).toBeVisible();
    expect(card).toHaveTextContent("uniwrt local CA 1a2b");
    for (const name of info.names) expect(within(card).getByText(name)).toBeVisible();
    expect(card).toHaveTextContent("Renewed 30 days before.");
    const download = within(card).getByRole("link", { name: "Download the CA" });
    expect(download).toHaveAttribute("href", "/api/v1/tls/ca.crt");
    expect(download).toHaveAttribute("download", "uniwrt-ca.crt");
    expect(within(card).getByRole("link", { name: "How to trust it" })).toHaveAttribute(
      "href",
      "/help/install#trust-the-controllers-certificate",
    );
    expect(within(card).getByRole("button", { name: "Copy the fingerprint" })).toBeVisible();
  });
});
