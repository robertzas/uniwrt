import { createContext } from "solid-js";
import type { UserInfo } from "../api/gen/UserInfo";

/** The signed-in user and the way out; provided by the app once signed in. */
export interface Session {
  readonly user: UserInfo;
  signOut: () => Promise<void>;
}

export const SessionContext = createContext<Session>();

/** A new password's least length, as the server's policy (`AuthState.min_password_chars`). */
export const MIN_PASSWORD_CHARS = 12;

/** Why `password` can't be a new password, if it can't: the server checks it again. */
export function newPasswordError(password: string, tooShort: (count: number) => string) {
  return [...password].length < MIN_PASSWORD_CHARS ? tooShort(MIN_PASSWORD_CHARS) : undefined;
}
