import { describe, expect, it, vi } from "vitest";
import { ApiError, api, onSessionEnded } from "./api";

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

  it("sends JSON with the CSRF header on state-changing requests", async () => {
    const user = { id: 1, username: "admin", role: "admin" };
    const fetch = vi.fn(async () => Response.json(user));
    vi.stubGlobal("fetch", fetch);

    await expect(api.auth.login({ username: "admin", password: "pw" })).resolves.toEqual(user);
    const [url, init] = fetch.mock.calls[0] as unknown as [string, RequestInit];
    const headers = new Headers(init.headers);
    expect(url).toBe("/api/v1/auth/login");
    expect(init.method).toBe("POST");
    expect(headers.get("x-uniwrt-csrf")).toBe("1");
    expect(headers.get("content-type")).toBe("application/json");
    expect(JSON.parse(String(init.body))).toEqual({ username: "admin", password: "pw" });

    await api.health();
    const [, get] = fetch.mock.calls[1] as unknown as [string, RequestInit];
    expect(new Headers(get.headers).has("x-uniwrt-csrf")).toBe(false);
  });

  it("reports an ended session on 401, except for sign-in", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(null, { status: 401 })),
    );
    const ended = vi.fn();
    const off = onSessionEnded(ended);

    await expect(api.auth.login({ username: "a", password: "b" })).rejects.toThrow();
    expect(ended).not.toHaveBeenCalled();
    await expect(api.nodes.list()).rejects.toMatchObject({ status: 401 });
    expect(ended).toHaveBeenCalledOnce();
    off();
  });

  it("reads Retry-After and accepts empty answers", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(null, { status: 429, headers: { "retry-after": "30" } })),
    );
    await expect(api.auth.login({ username: "a", password: "b" })).rejects.toMatchObject({
      status: 429,
      retryAfter: 30,
    });
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(null, { status: 204 })),
    );
    await expect(api.auth.logout()).resolves.toBeUndefined();
  });
});
