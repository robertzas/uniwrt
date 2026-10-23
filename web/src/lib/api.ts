import type { Health } from "../api/gen/Health";

/** A failed API call. Problem details (RFC 9457) from the server fill `title` and `message`. */
export class ApiError extends Error {
  override readonly name = "ApiError";

  constructor(
    readonly status: number,
    readonly title: string,
    detail?: string,
  ) {
    super(detail ?? title);
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  headers.set("accept", "application/json");
  const response = await fetch(`/api/v1${path}`, { ...init, headers });
  if (!response.ok) {
    throw await toError(response);
  }
  return (await response.json()) as T;
}

async function toError(response: Response): Promise<ApiError> {
  if (response.headers.get("content-type")?.startsWith("application/problem+json")) {
    const problem = (await response.json()) as { title?: string; detail?: string };
    return new ApiError(response.status, problem.title ?? response.statusText, problem.detail);
  }
  return new ApiError(response.status, response.statusText || `HTTP ${response.status}`);
}

/** The typed API client; components reach the server only through this (CONVENTIONS.md). */
export const api = {
  health: (signal?: AbortSignal) => request<Health>("/health", signal ? { signal } : {}),
};
