import { ExternalLink, Hammer, LoaderCircle, TriangleAlert } from "lucide";
import { createEffect, createSignal, For, Match, Show, Switch } from "solid-js";
import type { FirmwareOffer } from "../../api/gen/FirmwareOffer";
import type { ImageBuild } from "../../api/gen/ImageBuild";
import type { UpgradePlan } from "../../api/gen/UpgradePlan";
import { api, errorText } from "../../lib/api";
import { defaultOffer, followBuild, tightTmp } from "../../lib/firmware";
import { formatBytes, formatTime } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { BuildFailed, BuildRunning, ImageRow } from "../../ui/ImageBuild";
import { JobSuccessNote, RequestError } from "../../ui/JobOutcome";
import { RadioCards } from "../../ui/RadioCards";

const t = strings.firmware;
const d = t.dialog;

/**
 * Prepares a node's upgrade image (D-121): a release on offer, the plan worked out from the
 * node's packages read again (what is kept, renamed, added, dropped), then ASU's build followed
 * until its sysupgrade image is ready. Nothing changes on the node.
 */
export function UpgradeDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  node: number;
  name: string;
  offers: readonly FirmwareOffer[];
}) {
  const [release, setRelease] = createSignal<string>();
  const [plan, setPlan] = createSignal<UpgradePlan>();
  const [planError, setPlanError] = createSignal<string>();
  const [build, setBuild] = createSignal<ImageBuild>();
  const [buildError, setBuildError] = createSignal<string>();
  const [starting, setStarting] = createSignal(false);
  const chosen = () => release() ?? defaultOffer(props.offers)?.release;
  const offer = () => props.offers.find((known) => known.release === chosen());

  // The plan of the chosen release, read again each time the dialog opens or the release changes.
  createEffect(
    () => (props.open ? chosen() : undefined),
    (wanted) => {
      setPlan(undefined);
      setPlanError(undefined);
      setBuild(undefined);
      setBuildError(undefined);
      if (!wanted) return;
      const controller = new AbortController();
      api.firmware
        .plan(props.node, { release: wanted }, controller.signal)
        .then((found) => {
          setPlan(found);
        })
        .catch((err) => {
          if (!controller.signal.aborted) setPlanError(errorText(err));
        });
      return () => controller.abort();
    },
  );
  followBuild({
    build,
    active: () => props.open,
    fetch: (hash, signal) => api.firmware.builds.get(hash, signal),
    onBuild: setBuild,
    onError: setBuildError,
  });

  const start = async () => {
    const wanted = chosen();
    if (!wanted) return;
    setStarting(true);
    setBuildError(undefined);
    try {
      const started = await api.firmware.builds.create(props.node, { release: wanted });
      setBuild(started.build);
    } catch (err) {
      setBuildError(errorText(err));
    } finally {
      setStarting(false);
    }
  };
  const sysupgrade = () => {
    const images = build()?.images ?? [];
    const own = images.filter((image) => image.kind === "sysupgrade");
    return own.length > 0 ? own : images;
  };

  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={d.title(props.name)}
      description={d.description}
      size="lg"
    >
      <div class="flex flex-col gap-4 px-5 pb-5">
        <Show when={props.offers.length > 1}>
          <RadioCards
            label={d.release}
            value={chosen() ?? ""}
            onChange={(value) => {
              setRelease(value);
            }}
            disabled={build() !== undefined || starting()}
            options={props.offers.map((known) => ({
              value: known.release,
              label: `OpenWrt ${known.release} · ${t.kind[known.kind] ?? known.kind}`,
              description: t.kindHint[known.kind] ?? "",
            }))}
          />
        </Show>
        <Show when={offer()}>
          {(current) => (
            <a
              href={current().notes}
              target="_blank"
              rel="noopener noreferrer"
              class="inline-flex w-fit items-center gap-1.5 text-accent text-sm hover:underline"
            >
              {t.notes} · OpenWrt {current().release}
              <Icon icon={ExternalLink} class="size-3.5" />
            </a>
          )}
        </Show>
        <RequestError text={planError()} />
        <Show
          when={plan()}
          fallback={
            <Show when={!planError()}>
              <p role="status" class="flex items-center gap-2 text-fg-muted text-sm">
                <Icon icon={LoaderCircle} class="size-4 animate-spin text-accent" />
                {d.reading}
              </p>
            </Show>
          }
        >
          {(found) => <PlanView plan={found()} />}
        </Show>
        <RequestError text={buildError()} />
        <Show when={build()}>
          {(current) => (
            <Switch>
              <Match when={current().state === "queued" || current().state === "building"}>
                <BuildRunning build={current()} />
              </Match>
              <Match when={current().state === "done"}>
                <JobSuccessNote text={d.ready} />
                <ul class="flex flex-col gap-2" aria-label={strings.ztp.setup.image.state.done}>
                  <For each={sysupgrade()}>
                    {(image) => <ImageRow image={image} size={formatBytes(image.size_bytes)} />}
                  </For>
                </ul>
              </Match>
              <Match when={current().state === "failed"}>
                <BuildFailed build={current()} />
              </Match>
            </Switch>
          )}
        </Show>
        <div class="flex justify-end gap-2">
          <Button size="sm" onClick={() => props.onOpenChange(false)}>
            {d.close}
          </Button>
          <Show when={build() === undefined || build()?.state === "failed"}>
            <Button
              size="sm"
              variant="primary"
              icon={Hammer}
              disabled={plan() === undefined || starting()}
              onClick={start}
            >
              {starting() ? d.building : build() ? d.again : d.build}
            </Button>
          </Show>
        </div>
      </div>
    </Dialog>
  );
}

/** What the image holds against the node, the dropped packages as a warning. */
function PlanView(props: { plan: UpgradePlan }) {
  const tight = () => tightTmp(props.plan);
  return (
    <section class="flex flex-col gap-3" aria-label={d.kept(props.plan.kept.length)}>
      <Show when={props.plan.dropped.length > 0}>
        <div role="alert" class="rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5">
          <p class="flex items-center gap-2 font-medium text-fg text-sm">
            <Icon icon={TriangleAlert} class="size-4 shrink-0 text-warn" />
            {d.dropped(props.plan.release)}
          </p>
          <Names names={props.plan.dropped} />
          <p class="mt-1.5 text-fg-muted text-xs">{d.droppedHint}</p>
        </div>
      </Show>
      <Group label={d.kept(props.plan.kept.length)} names={props.plan.kept} />
      <Show when={props.plan.renamed.length > 0}>
        <div>
          <p class="text-fg-muted text-xs">{d.renamed}</p>
          <ul class="mt-1 flex flex-wrap gap-1.5">
            <For each={props.plan.renamed}>
              {(rename) => (
                <li class="rounded-control border border-edge bg-canvas px-1.5 py-0.5 font-mono text-fg text-xs">
                  {rename.from} → {rename.to}
                </li>
              )}
            </For>
          </ul>
        </div>
      </Show>
      <Group label={d.added} names={props.plan.added} />
      <Group label={d.removed} names={props.plan.removed} />
      <Show when={props.plan.image_bytes !== null}>
        <p class="text-fg-subtle text-xs">
          {d.size(
            formatBytes(props.plan.image_bytes),
            props.plan.tmp_free_bytes === null ? null : formatBytes(props.plan.tmp_free_bytes),
          )}
        </p>
      </Show>
      <Show when={tight()}>
        <p class="flex items-center gap-2 text-sm text-warn">
          <Icon icon={TriangleAlert} class="size-4 shrink-0" />
          {d.tight}
        </p>
      </Show>
      <p class="text-fg-subtle text-xs">{d.read(formatTime(props.plan.packages_at))}</p>
    </section>
  );
}

/** A labelled list of packages, left out when empty. */
function Group(props: { label: string; names: readonly string[] }) {
  return (
    <Show when={props.names.length > 0}>
      <div>
        <p class="text-fg-muted text-xs">{props.label}</p>
        <Names names={props.names} />
      </div>
    </Show>
  );
}

function Names(props: { names: readonly string[] }) {
  return (
    <ul class="mt-1 flex flex-wrap gap-1.5">
      <For each={props.names}>
        {(name) => (
          <li class="rounded-control border border-edge bg-canvas px-1.5 py-0.5 font-mono text-fg text-xs">
            {name}
          </li>
        )}
      </For>
    </ul>
  );
}
