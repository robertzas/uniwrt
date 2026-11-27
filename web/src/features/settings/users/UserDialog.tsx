import { createSignal, Show } from "solid-js";
import type { Account } from "../../../api/gen/Account";
import type { UserInfo } from "../../../api/gen/UserInfo";
import type { UserPatch } from "../../../api/gen/UserPatch";
import type { UserRole } from "../../../api/gen/UserRole";
import { api, errorText } from "../../../lib/api";
import { MIN_PASSWORD_CHARS, newPasswordError } from "../../../lib/session";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Dialog, DialogFooter } from "../../../ui/Dialog";
import { RequestError } from "../../../ui/JobOutcome";
import { RadioCards } from "../../../ui/RadioCards";
import { TextField } from "../../../ui/TextField";

const t = strings.settings.users;
const ROLES: readonly UserRole[] = ["viewer", "admin"];

/**
 * Adds a user (no `user`) or edits one: the role, and a new password the admin hands over (not
 * their own: the account menu asks for the current one). The only admin's role is fixed.
 * `onSaved` says whether anything changed, which signed the user out.
 */
export function UserDialog(props: {
  user: Account | undefined;
  self: boolean;
  onlyAdmin: boolean;
  onClose: () => void;
  onSaved: (user: UserInfo, changed: boolean) => void;
}) {
  const [username, setUsername] = createSignal("");
  const [role, setRole] = createSignal<UserRole>(props.user?.role ?? "viewer");
  const [password, setPassword] = createSignal("");
  const [pending, setPending] = createSignal(false);
  const [error, setError] = createSignal<string>();
  const [passwordError, setPasswordError] = createSignal<string>();

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    if (pending()) return;
    setError(undefined);
    const user = props.user;
    const tooShort =
      user === undefined || password() !== ""
        ? newPasswordError(password(), strings.account.password.tooShort)
        : undefined;
    setPasswordError(tooShort);
    if (tooShort) return;
    setPending(true);
    try {
      if (user === undefined) {
        const saved = await api.users.create({
          username: username(),
          password: password(),
          role: role(),
        });
        props.onSaved(saved, true);
        return;
      }
      const patch: UserPatch = {};
      if (role() !== user.role) patch.role = role();
      if (password() !== "") patch.password = password();
      if (patch.role === undefined && patch.password === undefined) {
        props.onClose();
        return;
      }
      props.onSaved(await api.users.update(user.id, patch), true);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) props.onClose();
      }}
      title={props.user ? t.dialog.editTitle(props.user.username) : t.dialog.addTitle}
      description={props.user ? t.dialog.editDescription : t.dialog.addDescription}
    >
      <form class="flex flex-col gap-4 px-5 pt-2 pb-4" onSubmit={submit}>
        <RequestError text={error()} />
        <Show when={!props.user}>
          <TextField
            label={t.dialog.username}
            name="username"
            autocomplete="off"
            value={username()}
            onInput={setUsername}
            hint={t.dialog.usernameHint}
            required
          />
        </Show>
        <RadioCards
          label={t.dialog.role}
          value={role()}
          options={ROLES.map((value) => ({
            value,
            label: strings.account.role[value],
            description: t.roles[value],
          }))}
          onChange={setRole}
          disabled={props.onlyAdmin}
        />
        <Show when={props.onlyAdmin}>
          <p class="-mt-2 text-fg-subtle text-xs">{t.dialog.lastAdmin}</p>
        </Show>
        <Show
          when={!props.self}
          fallback={<p class="text-fg-subtle text-sm">{t.dialog.ownPassword}</p>}
        >
          <TextField
            label={props.user ? t.dialog.newPassword : t.dialog.password}
            name="new-password"
            type="password"
            autocomplete="new-password"
            value={password()}
            onInput={(value) => {
              setPassword(value);
              setPasswordError(undefined);
            }}
            placeholder={props.user ? t.dialog.keepPassword : undefined}
            hint={strings.account.password.minChars(MIN_PASSWORD_CHARS)}
            error={passwordError()}
            required={!props.user}
          />
        </Show>
        <DialogFooter class="pt-2">
          <Button variant="ghost" type="button" onClick={() => props.onClose()}>
            {t.dialog.cancel}
          </Button>
          <Button variant="primary" type="submit" disabled={pending()}>
            {pending() ? t.dialog.pending : props.user ? t.dialog.save : t.dialog.add}
          </Button>
        </DialogFooter>
      </form>
    </Dialog>
  );
}
