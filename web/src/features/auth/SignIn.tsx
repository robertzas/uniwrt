import type { JSX } from "@solidjs/web";
import { Info, LockKeyhole } from "lucide";
import { createSignal, Match, Show, Switch } from "solid-js";
import type { AuthState } from "../../api/gen/AuthState";
import type { UserInfo } from "../../api/gen/UserInfo";
import { ApiError, api } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { TextField } from "../../ui/TextField";
import { Wordmark } from "../../ui/Wordmark";

export interface SignInProps {
  state: AuthState;
  onSignedIn: (user: UserInfo) => void;
  /** Reloads the auth state (setup closed). */
  onRetry: () => void;
  /** Shown above the form, e.g. that the session ended. */
  notice?: string | undefined;
}

/**
 * The signed-out app: first-run setup while it is open, a note when it closed without an admin,
 * and the login form otherwise (security.md, D-040).
 */
export function SignIn(props: SignInProps) {
  return (
    <AuthFrame>
      <Switch fallback={<LoginForm {...props} />}>
        <Match when={props.state.setup === "open"}>
          <SetupForm {...props} />
        </Match>
        <Match when={props.state.setup === "closed"}>
          <SetupClosed onRetry={props.onRetry} />
        </Match>
      </Switch>
    </AuthFrame>
  );
}

/** A centered card under the wordmark, for the screens shown before the shell. */
export function AuthFrame(props: { children: JSX.Element }) {
  return (
    <main class="flex min-h-dvh flex-col items-center justify-center bg-canvas px-4 py-10">
      <div class="mb-8">
        <Wordmark />
      </div>
      <Card class="w-full max-w-sm p-6 sm:p-8">{props.children}</Card>
    </main>
  );
}

function Heading(props: { title: string; description: string }) {
  return (
    <header class="mb-6">
      <h1 class="font-semibold text-xl tracking-tight">{props.title}</h1>
      <p class="mt-1.5 text-fg-muted text-sm leading-relaxed">{props.description}</p>
    </header>
  );
}

function message(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 429 && error.retryAfter) return strings.auth.tooMany(error.retryAfter);
    return error.message;
  }
  return strings.auth.unreachable.title;
}

/** Submits once at a time and keeps the server's answer as the form's error. */
function createSubmit(run: () => Promise<UserInfo>, done: (user: UserInfo) => void) {
  const [pending, setPending] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (pending()) return;
    setPending(true);
    setError(undefined);
    try {
      done(await run());
    } catch (e) {
      setError(message(e));
    } finally {
      setPending(false);
    }
  };
  return { pending, error, setError, submit };
}

function FormError(props: { text: string | undefined }) {
  return (
    <Show when={props.text}>
      {(text) => (
        <p
          role="alert"
          class="mb-4 flex items-start gap-2 rounded-control border border-crit/40 bg-crit/10 px-3 py-2 text-fg text-sm"
        >
          <Icon icon={LockKeyhole} class="mt-0.5 size-4 shrink-0 text-crit" />
          {text()}
        </p>
      )}
    </Show>
  );
}

function LoginForm(props: SignInProps) {
  const text = strings.auth;
  const [username, setUsername] = createSignal("");
  const [password, setPassword] = createSignal("");
  const form = createSubmit(
    () => api.auth.login({ username: username(), password: password() }),
    (user) => props.onSignedIn(user),
  );

  return (
    <>
      <Heading title={text.signIn.title} description={text.signIn.description} />
      <Show when={props.notice}>
        {(notice) => (
          <p
            role="status"
            class="mb-4 flex items-start gap-2 rounded-control border border-edge bg-raised px-3 py-2 text-sm"
          >
            <Icon icon={Info} class="mt-0.5 size-4 shrink-0 text-info" />
            {notice()}
          </p>
        )}
      </Show>
      <FormError text={form.error()} />
      <form class="flex flex-col gap-4" onSubmit={form.submit}>
        <TextField
          label={text.username}
          name="username"
          autocomplete="username"
          value={username()}
          onInput={setUsername}
          required
          autofocus
        />
        <TextField
          label={text.password}
          name="password"
          type="password"
          autocomplete="current-password"
          value={password()}
          onInput={setPassword}
          required
        />
        <Button type="submit" variant="primary" class="mt-2 h-10" disabled={form.pending()}>
          {form.pending() ? text.signIn.pending : text.signIn.submit}
        </Button>
      </form>
    </>
  );
}

function SetupForm(props: SignInProps) {
  const text = strings.auth;
  const [username, setUsername] = createSignal("admin");
  const [password, setPassword] = createSignal("");
  const [confirm, setConfirm] = createSignal("");
  const [touched, setTouched] = createSignal(false);
  const mismatch = () => touched() && confirm() !== password();
  const form = createSubmit(
    () => api.auth.setup({ username: username().trim(), password: password() }),
    (user) => props.onSignedIn(user),
  );
  const submit = (event: SubmitEvent) => {
    setTouched(true);
    if (confirm() !== password()) {
      event.preventDefault();
      return;
    }
    void form.submit(event);
  };
  const minutes = () => Math.max(1, Math.ceil((props.state.setup_closes_in_s ?? 0) / 60));

  return (
    <>
      <Heading title={text.setup.title} description={text.setup.description} />
      <FormError text={form.error()} />
      <form class="flex flex-col gap-4" onSubmit={submit}>
        <TextField
          label={text.username}
          name="username"
          autocomplete="username"
          value={username()}
          onInput={setUsername}
          required
        />
        <TextField
          label={text.password}
          name="new-password"
          type="password"
          autocomplete="new-password"
          value={password()}
          onInput={setPassword}
          hint={text.setup.minChars(props.state.min_password_chars)}
          minlength={props.state.min_password_chars}
          required
          autofocus
        />
        <TextField
          label={text.setup.confirm}
          name="confirm-password"
          type="password"
          autocomplete="new-password"
          value={confirm()}
          onInput={setConfirm}
          error={mismatch() ? text.setup.mismatch : undefined}
          required
        />
        <Button type="submit" variant="primary" class="mt-2 h-10" disabled={form.pending()}>
          {form.pending() ? text.setup.pending : text.setup.submit}
        </Button>
        <Show when={props.state.setup_closes_in_s !== null}>
          <p class="text-center text-fg-subtle text-xs">{text.setup.closesIn(minutes())}</p>
        </Show>
      </form>
    </>
  );
}

function SetupClosed(props: { onRetry: () => void }) {
  const text = strings.auth.closed;
  return (
    <>
      <Heading title={text.title} description={text.description} />
      <Button class="w-full" onClick={() => props.onRetry()}>
        {text.retry}
      </Button>
    </>
  );
}
