import { Download, ShieldCheck, Terminal } from "lucide";
import { createEffect, createSignal, Match, Show, Switch } from "solid-js";
import type { ZtpOneLiner } from "../../../api/gen/ZtpOneLiner";
import { api, errorText } from "../../../lib/api";
import { relative } from "../../../lib/rf";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Dialog } from "../../../ui/Dialog";
import { Icon } from "../../../ui/Icon";
import { JobSuccessNote, RequestError } from "../../../ui/JobOutcome";
import { Tab, TabList, TabPanel, Tabs } from "../../../ui/Tabs";
import { TextField } from "../../../ui/TextField";
import { RolePicker } from "./ZtpDialogs";

const t = strings.ztp.setup;
const o = t.oneLiner;
/** How often a one-liner's state is read while the device hasn't called home. */
const FOLLOW_MS = 2_000;

type SetupRole = "ap" | "bridge" | "gateway";

const role = strings.devices.role;
const oneLinerRoles = [
  { value: "ap", label: role.ap },
  { value: "bridge", label: role.bridge },
] as const;
const scriptRoles = [...oneLinerRoles, { value: "gateway", label: role.gateway }] as const;

/**
 * Setting up a new device to call home (adoption.md, Paths B and D): a one-liner to paste into
 * a running OpenWrt, followed until the device calls home, or the first-boot script for an
 * image. `onCalledHome` refreshes the inbox once a one-liner was used.
 */
export function SetupDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCalledHome: () => void;
}) {
  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={t.title}
      description={t.description}
      size="lg"
    >
      <div class="px-5 pt-1 pb-5">
        <Tabs defaultValue="one-liner" aria-label={t.title}>
          <TabList>
            <Tab value="one-liner">{t.tabs.oneLiner}</Tab>
            <Tab value="script">{t.tabs.script}</Tab>
          </TabList>
          <TabPanel value="one-liner" class="pt-4">
            <OneLiner open={props.open} onCalledHome={props.onCalledHome} />
          </TabPanel>
          <TabPanel value="script" class="pt-4">
            <Script />
          </TabPanel>
        </Tabs>
        <div class="mt-4 flex justify-end">
          <Button onClick={() => props.onOpenChange(false)}>{t.close}</Button>
        </div>
      </div>
    </Dialog>
  );
}

function OneLiner(props: { open: boolean; onCalledHome: () => void }) {
  const [setupRole, setSetupRole] = createSignal<"ap" | "bridge">("ap");
  const [name, setName] = createSignal("");
  const [made, setMade] = createSignal<ZtpOneLiner>();
  const [error, setError] = createSignal<string>();
  const [pending, setPending] = createSignal(false);
  const [copied, setCopied] = createSignal(false);
  const [now, setNow] = createSignal(Date.now());

  // A clock for the countdown, while open.
  createEffect(
    () => props.open,
    (open) => {
      if (!open) return;
      setNow(Date.now());
      const timer = setInterval(() => setNow(Date.now()), 1_000);
      return () => clearInterval(timer);
    },
  );
  // Follows the command until the device calls home, or it expires.
  createEffect(
    () => {
      const one = made();
      const open = one?.state === "waiting" || one?.state === "fetched";
      return props.open && open ? one?.token : undefined;
    },
    (token) => {
      if (!token) return;
      const controller = new AbortController();
      const timer = setInterval(async () => {
        try {
          const fresh = await api.ztp.oneLiners.get(token, controller.signal);
          setMade(fresh);
          if (fresh.state === "used") props.onCalledHome();
        } catch {
          // The next read tries again.
        }
      }, FOLLOW_MS);
      return () => {
        controller.abort();
        clearInterval(timer);
      };
    },
  );

  const make = async () => {
    setPending(true);
    setError(undefined);
    setCopied(false);
    try {
      const trimmed = name().trim();
      setMade(
        await api.ztp.oneLiners.create({ role: setupRole(), ...(trimmed && { name: trimmed }) }),
      );
    } catch (err) {
      setError(errorText(err));
    } finally {
      setPending(false);
    }
  };
  const withdraw = async (token: string) => {
    try {
      await api.ztp.oneLiners.withdraw(token);
      setMade(await api.ztp.oneLiners.get(token));
    } catch (err) {
      setError(errorText(err));
    }
  };
  const copy = async (command: string) => {
    await navigator.clipboard?.writeText(command);
    setCopied(true);
  };

  return (
    <div class="flex flex-col gap-4">
      <p class="text-fg-muted text-sm">{o.intro}</p>
      <RequestError text={error()} />
      <Show
        when={made()}
        fallback={
          <form
            class="flex flex-col gap-4"
            onSubmit={(event) => {
              event.preventDefault();
              void make();
            }}
          >
            <RolePicker
              label={t.role}
              value={setupRole()}
              options={oneLinerRoles}
              onChange={setSetupRole}
            />
            <TextField
              label={strings.ztp.form.name}
              name="name"
              value={name()}
              onInput={setName}
              hint={strings.ztp.form.nameHint}
              autocomplete="off"
            />
            <p class="text-fg-subtle text-xs">{o.noGateway}</p>
            <div>
              <Button type="submit" variant="primary" icon={Terminal} disabled={pending()}>
                {pending() ? o.making : o.make}
              </Button>
            </div>
          </form>
        }
      >
        {(one) => (
          <>
            <div class="rounded-control border border-edge bg-canvas">
              <pre
                class="max-h-40 overflow-auto whitespace-pre-wrap break-all px-3 py-2.5 font-mono text-fg text-xs"
                data-testid="one-liner"
              >
                {one().command}
              </pre>
              <div class="flex flex-wrap items-center gap-2 border-edge border-t px-3 py-2">
                <Button
                  size="sm"
                  variant="primary"
                  disabled={one().state === "expired"}
                  onClick={() => copy(one().command)}
                >
                  {copied() ? o.copied : o.copy}
                </Button>
                <Show when={one().state === "waiting" || one().state === "fetched"}>
                  <span class="text-fg-subtle text-xs">
                    {o.expires(relative(one().expires_at, now()))}
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    class="ml-auto"
                    onClick={() => withdraw(one().token)}
                  >
                    {o.withdraw}
                  </Button>
                </Show>
              </div>
            </div>
            <Switch>
              <Match when={one().state === "waiting"}>
                <p role="status" class="text-fg-muted text-sm">
                  {o.state.waiting}
                </p>
              </Match>
              <Match when={one().state === "fetched"}>
                <p role="status" class="text-fg-muted text-sm">
                  {o.state.fetched(one().fetched_from ?? "")}
                </p>
              </Match>
              <Match when={one().state === "used"}>
                <JobSuccessNote text={o.state.used(one().device ?? "")} />
              </Match>
              <Match when={one().state === "expired"}>
                <p role="status" class="text-fg-muted text-sm">
                  {o.state.expired}
                </p>
              </Match>
            </Switch>
            <ul class="flex flex-col gap-1.5 text-fg-subtle text-xs">
              <li class="flex gap-1.5">
                <Icon icon={ShieldCheck} class="mt-0.5 size-3.5 shrink-0 text-ok" />
                {o.safety}
              </li>
              <li class="flex gap-1.5">
                <Icon icon={Terminal} class="mt-0.5 size-3.5 shrink-0" />
                {o.after}
              </li>
            </ul>
            <div>
              <Button size="sm" onClick={() => setMade(undefined)}>
                {o.another}
              </Button>
            </div>
          </>
        )}
      </Show>
    </div>
  );
}

function Script() {
  const [scriptRole, setScriptRole] = createSignal<SetupRole>("ap");
  return (
    <div class="flex flex-col gap-4">
      <p class="text-fg-muted text-sm">{t.script.intro}</p>
      <RolePicker
        label={t.role}
        value={scriptRole()}
        options={scriptRoles}
        onChange={setScriptRole}
      />
      <div>
        <a
          href={api.ztp.scriptUrl(scriptRole())}
          download="99-uniwrt"
          class="inline-flex h-8 items-center gap-2 rounded-control bg-accent-fill px-3 text-on-accent text-sm hover:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
        >
          <Icon icon={Download} class="size-4" />
          {t.script.download}
        </a>
      </div>
      <p class="text-fg-subtle text-xs">{t.script.noSecrets}</p>
    </div>
  );
}
