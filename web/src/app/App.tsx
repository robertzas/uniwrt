import { QueryClient, QueryClientProvider, useQuery, useQueryClient } from "@tanstack/solid-query";
import { createSignal, Loading, Match, onSettled, Show, Switch } from "solid-js";
import type { AuthState } from "../api/gen/AuthState";
import type { UserInfo } from "../api/gen/UserInfo";
import { AuthFrame, SignIn } from "../features/auth/SignIn";
import { api, onSessionEnded } from "../lib/api";
import { createLive, LiveContext } from "../lib/live";
import { type Session, SessionContext } from "../lib/session";
import { strings } from "../lib/strings";
import { LiveSocket } from "../lib/ws";
import { Button } from "../ui/Button";
import { createAppRouter } from "./routes";
import { Shell } from "./Shell";

const Router = createAppRouter();

export function App() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { staleTime: 10_000 } },
  });
  return (
    <QueryClientProvider client={queryClient}>
      <Gate />
    </QueryClientProvider>
  );
}

const AUTH = ["auth"] as const;

/**
 * Shows the sign-in screens until there is a session, then the app. When the session ends (a 401
 * from the API, or the hub closing with 4401), it drops the app's data and asks to sign in again.
 */
export function Gate() {
  const queryClient = useQueryClient();
  const [notice, setNotice] = createSignal<string>();
  const auth = useQuery(() => ({
    queryKey: AUTH,
    queryFn: ({ signal }) => api.auth.state(signal),
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  }));

  const setUser = (user: UserInfo | null) => {
    queryClient.setQueryData<AuthState>(AUTH, (state) =>
      state ? { ...state, user, ...(user && { setup: "done", setup_closes_in_s: null }) } : state,
    );
    if (!user) {
      queryClient.removeQueries({ predicate: (query) => query.queryKey[0] !== AUTH[0] });
    }
  };
  const ended = () => {
    if (!queryClient.getQueryData<AuthState>(AUTH)?.user) return;
    setNotice(strings.account.sessionEnded);
    setUser(null);
  };
  onSettled(() => onSessionEnded(ended));

  return (
    <Switch fallback={<Splash />}>
      <Match when={auth.isError}>
        <AuthFrame>
          <h1 class="font-semibold text-xl tracking-tight">{strings.auth.unreachable.title}</h1>
          <p class="mt-1.5 mb-6 text-fg-muted text-sm">{strings.auth.unreachable.description}</p>
          <Button class="w-full" onClick={() => auth.refetch()}>
            {strings.auth.unreachable.retry}
          </Button>
        </AuthFrame>
      </Match>
      <Match when={auth.isSuccess}>
        <Loading fallback={<Splash />}>
          <Show
            when={auth.data?.user}
            fallback={
              <Show when={auth.data}>
                {(state) => (
                  <SignIn
                    state={state()}
                    notice={notice()}
                    onSignedIn={(user) => {
                      setNotice(undefined);
                      setUser(user);
                    }}
                    onRetry={() => auth.refetch()}
                  />
                )}
              </Show>
            }
          >
            {(user) => (
              <SignedIn
                user={user()}
                onSignOut={() => setUser(null)}
                onSessionEnded={() => ended()}
              />
            )}
          </Show>
        </Loading>
      </Match>
    </Switch>
  );
}

function Splash() {
  return (
    <div role="status" class="grid min-h-dvh place-items-center bg-canvas">
      <span class="sr-only">{strings.auth.loading}</span>
      <span class="size-2 animate-pulse rounded-full bg-fg-subtle" aria-hidden="true" />
    </div>
  );
}

/** The app for a signed-in user: one live socket for as long as the session lasts. */
function SignedIn(props: { user: UserInfo; onSignOut: () => void; onSessionEnded: () => void }) {
  const socket = new LiveSocket({ onSessionEnded: () => props.onSessionEnded() });
  onSettled(() => () => socket.close());
  const live = createLive(socket);
  const session: Session = {
    get user() {
      return props.user;
    },
    async signOut() {
      socket.close();
      await api.auth.logout().catch(() => undefined);
      props.onSignOut();
    },
  };

  return (
    <SessionContext value={session}>
      <LiveContext value={live}>
        <Router>{(routed) => <Shell>{routed.children}</Shell>}</Router>
      </LiveContext>
    </SessionContext>
  );
}
