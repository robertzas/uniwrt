import { Cable, Smartphone } from "lucide";
import { createSignal, Show } from "solid-js";
import type { Failover } from "../../api/gen/Failover";
import { errorText } from "../../lib/api";
import { strings } from "../../lib/strings";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHeader } from "../../ui/Card";
import { Checkbox } from "../../ui/Checkbox";
import { RequestError } from "../../ui/JobOutcome";
import { Segmented } from "../../ui/Segmented";
import { TextField } from "../../ui/TextField";
import { Saved } from "./Saved";
import {
  type FailoverDraft,
  type FailoverField,
  failoverDraft,
  failoverOf,
  saveWan,
} from "./uplink";

const t = strings.internet;
const f = t.failover;

/** The backup line in words. */
function summary(failover: Failover): string {
  const where =
    failover.link.kind === "usb"
      ? f.usbOn(failover.link.device)
      : f.portOn(failover.link.port, failover.link.vlan);
  return f.summary(where, f.protos[failover.proto.kind]);
}

/**
 * Failover on the Internet page (wan.md, Failover; D-076): the backup line, and for admins the
 * form, saved as a revision.
 */
export function FailoverCard(props: {
  failover: Failover | null;
  admin: boolean;
  gateway: number | undefined;
  /** An IPv6-only WAN has no IPv4 to back up. */
  ipv4: boolean;
}) {
  const [draft, setDraft] = createSignal<FailoverDraft>();
  const [errors, setErrors] = createSignal<Partial<Record<FailoverField, string>>>({});
  const [failure, setFailure] = createSignal<string>();
  const [busy, setBusy] = createSignal(false);
  const [saved, setSaved] = createSignal<{ revision: number; installs: boolean }>();

  const edit = () => {
    setDraft(failoverDraft(props.failover));
    setErrors({});
    setFailure(undefined);
    setSaved(undefined);
  };
  const change = (next: Partial<FailoverDraft>) => {
    setDraft((now) => (now ? { ...now, ...next } : now));
    setErrors({});
  };
  const save = async () => {
    const now = draft();
    if (!now) return;
    const { failover, errors: found } = failoverOf(now);
    if (Object.keys(found).length > 0) {
      setErrors(found);
      return;
    }
    setBusy(true);
    try {
      const revision = await saveWan(
        (wan) => ({ ...wan, failover }),
        failover ? f.message : f.messageOff,
        props.gateway,
      );
      setSaved({ revision, installs: failover !== null && props.failover === null });
      setDraft(undefined);
    } catch (err) {
      setFailure(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card aria-label={f.title}>
      <CardHeader
        title={f.title}
        description={f.description}
        actions={
          <Show when={props.admin && props.ipv4 && !draft()}>
            <Button size="sm" onClick={edit}>
              {t.edit}
            </Button>
          </Show>
        }
      />
      <div class="flex flex-col gap-3 px-4 pb-4">
        <Show
          when={draft()}
          fallback={
            <Show
              when={props.failover}
              fallback={<p class="text-fg-muted text-sm">{props.ipv4 ? f.off : f.noIpv4}</p>}
            >
              {(failover) => (
                <div class="flex flex-col gap-1.5 text-sm">
                  <div>
                    <Badge tone="ok" icon={failover().link.kind === "usb" ? Smartphone : Cable}>
                      {summary(failover())}
                    </Badge>
                  </div>
                  <p class="text-fg-subtle text-xs">{f.checks}</p>
                </div>
              )}
            </Show>
          }
        >
          {(form) => (
            <form
              class="flex flex-col gap-3"
              onSubmit={(event) => {
                event.preventDefault();
                void save();
              }}
            >
              <Checkbox checked={form().on} onChange={(on) => change({ on })} label={f.enable} />
              <Show when={form().on}>
                <Segmented
                  label={f.link}
                  value={form().link}
                  options={[
                    { value: "port", label: f.links.port },
                    { value: "usb", label: f.links.usb },
                  ]}
                  onChange={(link) => change({ link })}
                  class="self-start"
                />
                <Show
                  when={form().link === "port"}
                  fallback={
                    <TextField
                      label={f.device}
                      hint={f.deviceHint}
                      value={form().device}
                      onInput={(device) => change({ device })}
                      error={errors().device}
                    />
                  }
                >
                  <div class="grid gap-3 sm:grid-cols-2">
                    <TextField
                      label={f.port}
                      hint={f.portHint}
                      value={form().port}
                      onInput={(port) => change({ port })}
                      error={errors().port}
                    />
                    <TextField
                      label={f.vlan}
                      value={form().vlan}
                      onInput={(vlan) => change({ vlan })}
                      error={errors().vlan}
                    />
                  </div>
                </Show>
                <Segmented
                  label={f.proto}
                  value={form().proto}
                  options={[
                    { value: "dhcp", label: f.protos.dhcp },
                    { value: "static", label: f.protos.static },
                  ]}
                  onChange={(proto) => change({ proto })}
                  class="self-start"
                />
                <Show when={form().proto === "static"}>
                  <div class="grid gap-3 sm:grid-cols-3">
                    <TextField
                      label={f.address}
                      value={form().address}
                      onInput={(address) => change({ address })}
                      error={errors().address}
                    />
                    <TextField
                      label={f.gateway}
                      value={form().gateway}
                      onInput={(gateway) => change({ gateway })}
                      error={errors().gateway}
                    />
                    <TextField
                      label={f.dns}
                      hint={f.dnsHint}
                      value={form().dns}
                      onInput={(dns) => change({ dns })}
                    />
                  </div>
                </Show>
                <p class="text-fg-subtle text-xs">{f.checks}</p>
              </Show>
              <div class="flex justify-end gap-2">
                <Button variant="ghost" onClick={() => setDraft(undefined)} disabled={busy()}>
                  {t.cancel}
                </Button>
                <Button type="submit" variant="primary" disabled={busy()}>
                  {t.save}
                </Button>
              </div>
            </form>
          )}
        </Show>
        <RequestError text={failure()} />
        <Show when={saved()}>
          {(done) => <Saved revision={done().revision} installs={done().installs} />}
        </Show>
      </div>
    </Card>
  );
}
