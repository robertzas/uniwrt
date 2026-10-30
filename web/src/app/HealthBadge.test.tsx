import { render, screen } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import { describe, expect, it, vi } from "vitest";
import { HealthBadge } from "./HealthBadge";

function renderBadge() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(() => (
    <QueryClientProvider client={client}>
      <HealthBadge />
    </QueryClientProvider>
  ));
  return client;
}

describe("HealthBadge", () => {
  it("shows the version when the controller answers", async () => {
    const health = { status: "ok", version: "1.2.3", commit: "abc1234", built_at: "2026-10-22" };
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => Response.json(health)),
    );
    renderBadge();

    expect(await screen.findByText("Online")).toBeInTheDocument();
    expect(screen.getByText("v1.2.3")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveAttribute("title", "Commit abc1234, built 2026-10-22");
  });

  it("reports an unreachable controller", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        throw new TypeError("Failed to fetch");
      }),
    );
    renderBadge();

    expect(await screen.findByText("Unreachable")).toBeInTheDocument();
    expect(screen.queryByText(/^v\d/)).not.toBeInTheDocument();
  });

  it("recovers when the controller answers again", async () => {
    const health = { status: "ok", version: "1.2.3", commit: "abc1234", built_at: "2026-10-22" };
    const fetch = vi.fn(async (): Promise<Response> => {
      throw new TypeError("Failed to fetch");
    });
    vi.stubGlobal("fetch", fetch);
    const client = renderBadge();
    expect(await screen.findByText("Unreachable")).toBeInTheDocument();

    fetch.mockImplementation(async () => Response.json(health));
    await client.refetchQueries({ queryKey: ["health"] });
    expect(await screen.findByText("Online")).toBeInTheDocument();
    expect(screen.getByText("v1.2.3")).toBeInTheDocument();
  });
});
