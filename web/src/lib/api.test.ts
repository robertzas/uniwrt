import { describe, expect, it, vi } from "vitest";
import { ApiError, api } from "./api";

describe("api", () => {
  it("asks /api/v1 for JSON and returns the body", async () => {
    const health = {
      status: "ok",
      version: "1.2.3",
      commit: "abc",
      built_at: "2026-10-22T00:00:00Z",
    };
    const fetch = vi.fn(async () => Response.json(health));
    vi.stubGlobal("fetch", fetch);

    await expect(api.health()).resolves.toEqual(health);
    const [url, init] = fetch.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe("/api/v1/health");
    expect(new Headers(init.headers).get("accept")).toBe("application/json");
  });

  it("turns problem details into an ApiError", async () => {
    const problem = { type: "about:blank", title: "Not Found", status: 404, detail: "No route" };
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(JSON.stringify(problem), {
            status: 404,
            headers: { "content-type": "application/problem+json" },
          }),
      ),
    );

    const error = await api.health().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({ status: 404, title: "Not Found", message: "No route" });
  });

  it("falls back to the status for other errors", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("oops", { status: 502 })),
    );

    await expect(api.health()).rejects.toMatchObject({ status: 502, message: "HTTP 502" });
  });
});
