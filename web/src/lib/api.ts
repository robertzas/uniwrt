import type { AuthState } from "../api/gen/AuthState";
import type { Credentials } from "../api/gen/Credentials";
import type { Health } from "../api/gen/Health";
import type { NodeDetail } from "../api/gen/NodeDetail";
import type { NodeId } from "../api/gen/NodeId";
import type { NodeSummary } from "../api/gen/NodeSummary";
import type { UserInfo } from "../api/gen/UserInfo";

/** A failed API call. Problem details (RFC 9457) from the server fill `title` and `message`. */
export class ApiError extends Error {
  override readonly name = "ApiError";

  constructor(
    readonly status: number,
    readonly title: string,
    detail?: string,
    /** Seconds to wait before trying again, from `Retry-After` (429). */
    readonly retryAfter?: number,
  ) {
    super(detail ?? title);
  }
}

let sessionEnded: (() => void) | undefined;

/**
 * Called when a request that needs a session gets 401: the session expired or was revoked. The
 * app answers by showing the sign-in screen. Returns a function that removes the handler.
 */
export function onSessionEnded(handler: () => void): () => void {
  sessionEnded = handler;
  return () => {
    if (sessionEnded === handler) sessionEnded = undefined;
  };
}

interface Options {
  method?: "GET" | "POST";
  body?: unknown;
  signal?: AbortSignal | undefined;
  /** A 401 means the session ended (default); false for sign-in, whose 401 is a wrong password. */
  session?: boolean;
}

async function request<T>(path: string, options: Options = {}): Promise<T> {
  const method = options.method ?? "GET";
  const headers = new Headers({ accept: "application/json" });
  // Every state-changing request carries the CSRF header (security.md).
  if (method !== "GET") headers.set("x-uniwrt-csrf", "1");
  if (options.body !== undefined) headers.set("content-type", "application/json");
  const response = await fetch(`/api/v1${path}`, {
    method,
    headers,
    ...(options.body !== undefined && { body: JSON.stringify(options.body) }),
    ...(options.signal && { signal: options.signal }),
  });
  if (!response.ok) {
    if (response.status === 401 && options.session !== false) sessionEnded?.();
    throw await toError(response);
  }
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

async function toError(response: Response): Promise<ApiError> {
  const retry = Number(response.headers.get("retry-after"));
  const retryAfter = Number.isFinite(retry) && retry > 0 ? retry : undefined;
  if (response.headers.get("content-type")?.startsWith("application/problem+json")) {
    const problem = (await response.json()) as { title?: string; detail?: string };
    return new ApiError(
      response.status,
      problem.title ?? response.statusText,
      problem.detail,
      retryAfter,
    );
  }
  const title = response.statusText || `HTTP ${response.status}`;
  return new ApiError(response.status, title, undefined, retryAfter);
}

/** The typed API client; components reach the server only through this (CONVENTIONS.md). */
export const api = {
  health: (signal?: AbortSignal) => request<Health>("/health", { signal }),
  auth: {
    state: (signal?: AbortSignal) => request<AuthState>("/auth", { signal, session: false }),
    setup: (credentials: Credentials) =>
      request<UserInfo>("/auth/setup", { method: "POST", body: credentials, session: false }),
    login: (credentials: Credentials) =>
      request<UserInfo>("/auth/login", { method: "POST", body: credentials, session: false }),
    logout: () => request<void>("/auth/logout", { method: "POST", session: false }),
  },
  nodes: {
    list: (signal?: AbortSignal) => request<NodeSummary[]>("/nodes", { signal }),
    get: (id: NodeId, signal?: AbortSignal) => request<NodeDetail>(`/nodes/${id}`, { signal }),
  },
};
