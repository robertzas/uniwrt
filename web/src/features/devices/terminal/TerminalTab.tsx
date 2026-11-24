import { Plug, RotateCw, TriangleAlert, Unplug } from "lucide";
import { createMemo, Match, onSettled, Show, Switch, useContext } from "solid-js";
import type { Node } from "../../../api/gen/Node";
import { LiveContext } from "../../../lib/live";
import { SessionContext } from "../../../lib/session";
import { strings } from "../../../lib/strings";
import { StatusDot, type Tone } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Icon } from "../../../ui/Icon";
import { describeEnd } from "./connection";
import { type TerminalState, terminalSession } from "./session";

const t = strings.device.terminal;

const tones: Record<TerminalState["phase"], Tone> = {
  idle: "neutral",
  loading: "info",
  connecting: "info",
  open: "ok",
  ended: "neutral",
  failed: "crit",
};

/**
 * A device's Terminal tab (`?tab=terminal`; observability.md, Terminal; D-141): root's login
 * shell over the controller's SSH connection, for admins, observe mode included. The terminal
 * goes on while the admin looks at other tabs, and ends when they leave the device.
 */
export function TerminalTab(props: { node: Node }) {
  const session = useContext(SessionContext);
  return (
    <Show
      when={session.user.role === "admin"}
      fallback={
        <Card>
          <CardHeader title={t.title} />
          <p class="px-4 pb-4 text-fg-muted text-sm">{t.adminOnly}</p>
        </Card>
      }
    >
      <Console node={props.node} />
    </Show>
  );
}

function Console(props: { node: Node }) {
  const live = useContext(LiveContext);
  const terminal = createMemo(() => terminalSession(props.node.id));
  const state = () => terminal().state();
  const online = () =>
    live.fleet.nodes.find((node) => node.id === props.node.id)?.status.state !== "offline";
  const running = () => {
    const phase = state().phase;
    return phase === "loading" || phase === "connecting" || phase === "open";
  };
  let frame: HTMLDivElement | undefined;

  onSettled(() => {
    const session = terminal();
    if (!frame) return;
    session.attach(frame);
    return () => session.detach();
  });

  const status = () => {
    const now = state();
    return now.phase === "ended" ? describeEnd(now.end) : t.state[now.phase];
  };

  return (
    <Card aria-label={t.label(props.node.name)}>
      <CardHeader
        title={t.title}
        description={t.description(props.node.name)}
        actions={
          <>
            <span class="flex items-center gap-1.5" role="status">
              <StatusDot tone={tones[state().phase]} pulse={state().phase === "connecting"} />
              {status()}
            </span>
            <Show
              when={running()}
              fallback={
                <Button
                  size="sm"
                  variant="primary"
                  icon={state().phase === "idle" ? Plug : RotateCw}
                  disabled={!online()}
                  title={online() ? undefined : t.offline}
                  onClick={() => void terminal().connect()}
                >
                  {state().phase === "idle" ? t.open : t.reconnect}
                </Button>
              }
            >
              <Button
                size="sm"
                variant="secondary"
                icon={Unplug}
                disabled={state().phase === "loading"}
                onClick={() => terminal().disconnect()}
              >
                {t.disconnect}
              </Button>
            </Show>
          </>
        }
      />
      <p class="mx-4 mb-3 flex items-start gap-2 text-fg-muted text-xs" role="note">
        <Icon icon={TriangleAlert} class="mt-px size-3.5 shrink-0 text-warn" />
        {t.drift}
      </p>
      <Switch>
        <Match when={!online() && !running()}>
          <p class="mx-4 mb-3 text-fg-muted text-xs">{t.offline}</p>
        </Match>
        <Match when={state().phase === "failed" && state()}>
          {(failed) => (
            <p role="alert" class="mx-4 mb-3 text-crit text-sm">
              {t.failed((failed() as { error: string }).error)}
            </p>
          )}
        </Match>
      </Switch>
      <div class="relative mx-4 mb-4 overflow-hidden rounded-lg border border-edge bg-[#0d1117] p-2">
        <div ref={frame} class="h-[60vh] min-h-72" />
        <Show when={state().phase === "idle"}>
          <p class="absolute inset-0 grid place-items-center px-6 text-center text-[#8b949e] text-sm">
            {t.idle}
          </p>
        </Show>
      </div>
    </Card>
  );
}
