import { useContext } from "solid-js";
import { LiveContext } from "./live";
import { SessionContext } from "./session";
import { strings } from "./strings";

/** A next step an empty page links to (D-148); `ui/EmptyState` takes it as an action. */
export interface NextStep {
  label: string;
  href: string;
}

const s = strings.page.next;

export const ADOPT: NextStep = { label: s.adopt, href: "/devices?adopt=1" };
export const SET_UP_WIFI: NextStep = { label: s.wifi, href: "/wifi" };

/**
 * The next steps empty pages offer (D-148, features/ui.md, Interaction patterns). Each step needs
 * an admin, so a viewer gets none.
 */
export function useNextSteps() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const admin = () => session.user.role === "admin";
  return {
    /** Adopting while the fleet is read and empty, else `then` (if any). */
    adoptFirst(then?: NextStep): NextStep[] {
      if (!admin() || !live.fleet.ready) return [];
      if (live.fleet.nodes.length === 0) return [ADOPT];
      return then ? [then] : [];
    },
    /** For a page that needs the gateway: adopt it, or set up a new router (the first-run wizard). */
    gateway(): NextStep[] {
      if (!admin()) return [];
      return [
        { label: s.adoptGateway, href: "/devices?adopt=1" },
        { label: s.setUpRouter, href: "/setup" },
      ];
    },
  };
}
