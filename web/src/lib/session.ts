import { createContext } from "solid-js";
import type { UserInfo } from "../api/gen/UserInfo";

/** The signed-in user and the way out; provided by the app once signed in. */
export interface Session {
  readonly user: UserInfo;
  signOut: () => Promise<void>;
}

export const SessionContext = createContext<Session>();
