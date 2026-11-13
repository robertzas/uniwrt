import type { JSX } from "@solidjs/web";
import { Fingerprint } from "lucide";
import { createEffect, createSignal, Show } from "solid-js";
import type { Job } from "../../../api/gen/Job";
import type { NodeRole } from "../../../api/gen/NodeRole";
import type { PendingDevice } from "../../../api/gen/PendingDevice";
import { api, errorText } from "../../../lib/api";
import { normalizeMac } from "../../../lib/names";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { Dialog } from "../../../ui/Dialog";
import { Icon } from "../../../ui/Icon";
import { RequestError } from "../../../ui/JobOutcome";
import { Segmented } from "../../../ui/Segmented";
import { TextField } from "../../../ui/TextField";

const t = strings.ztp;
const f = t.form;

type RoleChoice = NodeRole | "image";

const roleOptions = [
  { value: "image", label: f.roleImage },
  { value: "ap", label: strings.devices.role.ap },
  { value: "bridge", label: strings.devices.role.bridge },
  { value: "gateway", label: strings.devices.role.gateway },
] as const;

/** A role choice with its label shown (`Segmented` keeps its legend for screen readers). */
export function RolePicker<T extends string>(props: {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
}) {
  return (
    <div>
      <p class="mb-1.5 font-medium text-fg text-sm" aria-hidden="true">
        {props.label}
      </p>
      <Segmented
        label={props.label}
        value={props.value}
        options={props.options}
        onChange={props.onChange}
        class="w-fit"
      />
    </div>
  );
}

/** The name and role fields both dialogs share. */
function NameAndRole(props: {
  name: string;
  onName: (name: string) => void;
  role: RoleChoice;
  onRole: (role: RoleChoice) => void;
}) {
  return (
    <>
      <TextField
        label={f.name}
        name="name"
        value={props.name}
        onInput={props.onName}
        hint={f.nameHint}
        autocomplete="off"
      />
      <div>
        <RolePicker
          label={f.role}
          value={props.role}
          options={roleOptions}
          onChange={props.onRole}
        />
        <Show when={props.role === "image"}>
          <p class="mt-1.5 text-fg-subtle text-xs">{f.roleImageHint}</p>
        </Show>
      </div>
    </>
  );
}

function Footer(props: { children: JSX.Element }) {
  return <div class="mt-2 flex flex-wrap justify-end gap-2">{props.children}</div>;
}

const chosenRole = (role: RoleChoice) => (role === "image" ? undefined : role);

/** Adopting a device from the inbox, once the owner has compared its host key. */
export function ZtpAdoptDialog(props: {
  device: PendingDevice | undefined;
  onClose: () => void;
  onStarted: (job: Job) => void;
}) {
  const [name, setName] = createSignal("");
  const [role, setRole] = createSignal<RoleChoice>("image");
  const [error, setError] = createSignal<string>();
  const [pending, setPending] = createSignal(false);
  createEffect(
    () => props.device?.mac,
    () => {
      setName("");
      setRole("image");
      setError(undefined);
    },
  );
  const submit = async (device: PendingDevice) => {
    setPending(true);
    setError(undefined);
    try {
      const trimmed = name().trim();
      props.onStarted(
        await api.ztp.adopt(device.mac, {
          ...(trimmed && { name: trimmed }),
          ...(chosenRole(role()) && { role: chosenRole(role()) }),
        }),
      );
    } catch (err) {
      setError(errorText(err));
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog
      open={props.device !== undefined}
      onOpenChange={(open) => {
        if (!open) props.onClose();
      }}
      title={t.confirm.title(props.device?.hostname || props.device?.mac || "")}
      description={t.confirm.description}
    >
      <Show when={props.device}>
        {(device) => (
          <form
            class="flex flex-col gap-4 px-5 pt-1 pb-5"
            onSubmit={(event) => {
              event.preventDefault();
              void submit(device());
            }}
          >
            <RequestError text={error()} />
            <div class="rounded-control border border-accent/40 bg-accent/5 px-3.5 py-3">
              <p class="flex items-center gap-1.5 text-fg-muted text-xs">
                <Icon icon={Fingerprint} class="size-3.5 text-accent" />
                {t.confirm.fingerprint} ·{" "}
                <span class="font-mono">
                  {device().mac} · {device().address}
                </span>
              </p>
              <p
                class="mt-1.5 break-all font-medium font-mono text-fg text-sm"
                data-testid="ztp-fingerprint"
              >
                {device().fingerprint}
              </p>
              <p class="mt-1 text-fg-subtle text-xs">
                {t.confirm.details(device().model, device().release)}
              </p>
            </div>
            <NameAndRole name={name()} onName={setName} role={role()} onRole={setRole} />
            <Footer>
              <Button onClick={() => props.onClose()}>{f.cancel}</Button>
              <Button type="submit" variant="primary" disabled={pending()}>
                {pending() ? t.confirm.starting : t.confirm.submit}
              </Button>
            </Footer>
          </form>
        )}
      </Show>
    </Dialog>
  );
}

/** Pre-approving a MAC: its first hello adopts it (single-use, 7 days). */
export function ApproveDialog(props: { open: boolean; onClose: () => void; onSaved: () => void }) {
  const [mac, setMac] = createSignal("");
  const [touched, setTouched] = createSignal(false);
  const [name, setName] = createSignal("");
  const [role, setRole] = createSignal<RoleChoice>("image");
  const [error, setError] = createSignal<string>();
  const [pending, setPending] = createSignal(false);
  createEffect(
    () => props.open,
    (open) => {
      if (!open) return;
      setMac("");
      setTouched(false);
      setName("");
      setRole("image");
      setError(undefined);
    },
  );
  const macError = () => (touched() && !normalizeMac(mac()) ? f.macInvalid : undefined);
  const submit = async () => {
    setTouched(true);
    const normalized = normalizeMac(mac());
    if (!normalized) return;
    setPending(true);
    setError(undefined);
    try {
      const trimmed = name().trim();
      await api.ztp.approve(normalized, {
        ...(trimmed && { name: trimmed }),
        ...(chosenRole(role()) && { role: chosenRole(role()) }),
      });
      props.onSaved();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setPending(false);
    }
  };

  return (
    <Dialog
      open={props.open}
      onOpenChange={(open) => {
        if (!open) props.onClose();
      }}
      title={t.approvals.add}
      description={t.approvals.description}
    >
      <form
        class="flex flex-col gap-4 px-5 pt-1 pb-5"
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <RequestError text={error()} />
        <TextField
          label={f.mac}
          name="mac"
          value={mac()}
          onInput={setMac}
          hint={f.macHint}
          error={macError()}
          autocomplete="off"
          required
          autofocus
        />
        <NameAndRole name={name()} onName={setName} role={role()} onRole={setRole} />
        <Footer>
          <Button onClick={() => props.onClose()}>{f.cancel}</Button>
          <Button type="submit" variant="primary" disabled={pending()}>
            {pending() ? f.approving : f.approve}
          </Button>
        </Footer>
      </form>
    </Dialog>
  );
}
