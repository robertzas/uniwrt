import { ExternalLink, Hammer, LoaderCircle } from "lucide";
import { createEffect, createSignal, For, Match, Show, Switch, untrack } from "solid-js";
import type { FirmwareOffer } from "../../api/gen/FirmwareOffer";
import type { FirmwareUpload } from "../../api/gen/FirmwareUpload";
import type { GitHubRelease } from "../../api/gen/GitHubRelease";
import type { ImageBuild } from "../../api/gen/ImageBuild";
import type { NodeSource } from "../../api/gen/NodeSource";
import type { PlanBody } from "../../api/gen/PlanBody";
import type { UpgradePlan } from "../../api/gen/UpgradePlan";
import { api, errorText } from "../../lib/api";
import {
  choiceKey,
  defaultOffer,
  followBuild,
  type ImageChoice,
  installBody,
  openwrtSource,
  planBody,
} from "../../lib/firmware";
import { formatBytes, formatDateTime } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Dialog, DialogFooter } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { BuildFailed, BuildRunning, ImageRow } from "../../ui/ImageBuild";
import { JobSuccessNote, RequestError } from "../../ui/JobOutcome";
import { RadioCards } from "../../ui/RadioCards";
import { Segmented } from "../../ui/Segmented";
import { createInstall, InstallButton, InstallStatus } from "./install";
import { PlanView } from "./PlanView";

const t = strings.firmware;
const d = t.dialog;

type Source = "asu" | "official";

/**
 * Prepares a node's upgrade (D-121, D-122): one of its GitHub repo's releases, an OpenWrt release
 * on offer (built by ASU with the node's packages, or OpenWrt's official image with them installed
 * after; the node's source picks which first), or an uploaded image made for its board. The plan is worked out (for a release, from the node's packages read again); ASU's
 * build is followed until its sysupgrade image is ready. Nothing changes on the node until an admin
 * installs the image on a managed node (D-124), which the dialog then follows.
 */
export function UpgradeDialog(props: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  node: number;
  name: string;
  offers: readonly FirmwareOffer[];
  uploads: readonly FirmwareUpload[];
  /** Its GitHub repo's releases with an image for it, and the repo. */
  releases: readonly GitHubRelease[];
  repo: string | null;
  source: NodeSource;
  /** Management is on: only then is the upgrade installed from here. */
  managed: boolean;
}) {
  const [picked, setPicked] = createSignal<string>();
  const [source, setSource] = createSignal<Source>(untrack(() => openwrtSource(props.source)));
  const [plan, setPlan] = createSignal<UpgradePlan>();
  const [planError, setPlanError] = createSignal<string>();
  const [build, setBuild] = createSignal<ImageBuild>();
  const [buildError, setBuildError] = createSignal<string>();
  const [starting, setStarting] = createSignal(false);
  const choices = (): ImageChoice[] => [
    ...props.releases.map((release) => ({ kind: "github" as const, tag: release.tag })),
    ...props.offers.map((offer) => ({ kind: "release" as const, release: offer.release })),
    ...props.uploads.map((upload) => ({ kind: "upload" as const, sha256: upload.sha256 })),
  ];
  const chosen = (): ImageChoice | undefined => {
    const all = choices();
    const wanted = picked();
    const first = defaultOffer(props.offers)?.release;
    return (
      all.find((choice) => choiceKey(choice) === wanted) ??
      all.find((choice) => choice.kind === "github") ??
      all.find((choice) => choice.kind === "release" && choice.release === first) ??
      all[0]
    );
  };
  const release = () => {
    const choice = chosen();
    return choice?.kind === "release" ? choice.release : undefined;
  };
  const offer = () => props.offers.find((known) => known.release === release());
  const githubRelease = () => {
    const choice = chosen();
    return choice?.kind === "github"
      ? props.releases.find((known) => known.tag === choice.tag)
      : undefined;
  };
  const buildable = () => release() !== undefined && source() === "asu";
  const install = createInstall({
    node: () => props.node,
    name: () => props.name,
    body: () => {
      const choice = chosen();
      return choice && plan() ? installBody(choice, source() === "official", build()) : undefined;
    },
    managed: () => props.managed,
  });

  // The plan of the chosen image, worked out again each time the dialog opens or the choice changes.
  createEffect(
    () => {
      const choice = props.open ? chosen() : undefined;
      // As text, so an equal request (the catalog read again) doesn't plan again.
      return choice && JSON.stringify(planBody(choice, source() === "official"));
    },
    (wanted) => {
      setPlan(undefined);
      setPlanError(undefined);
      setBuild(undefined);
      setBuildError(undefined);
      if (!wanted) return;
      const controller = new AbortController();
      api.firmware
        .plan(props.node, JSON.parse(wanted) as PlanBody, controller.signal)
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
    const wanted = release();
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
  const label = (choice: ImageChoice) => {
    if (choice.kind === "release") {
      const known = props.offers.find((o) => o.release === choice.release);
      return {
        label: `OpenWrt ${choice.release} · ${t.kind[known?.kind ?? ""] ?? known?.kind ?? ""}`,
        description: t.kindHint[known?.kind ?? ""] ?? "",
      };
    }
    if (choice.kind === "github") {
      const known = props.releases.find((r) => r.tag === choice.tag);
      const published = known?.published_at == null ? null : formatDateTime(known.published_at);
      return {
        label: choice.tag,
        description: d.githubHint(props.repo ?? "GitHub", published, known?.prerelease ?? false),
      };
    }
    const upload = props.uploads.find((u) => u.sha256 === choice.sha256);
    return {
      label: d.uploaded(upload?.name ?? choice.sha256.slice(0, 12)),
      description: d.uploadedHint(upload?.version ?? null, formatBytes(upload?.size_bytes)),
    };
  };
  const busy = () => build() !== undefined || starting() || install.busy();

  return (
    <Dialog
      open={props.open}
      onOpenChange={props.onOpenChange}
      title={d.title(props.name)}
      description={d.description}
      size="lg"
    >
      <div class="flex flex-col gap-4 px-5 pb-5">
        <Show when={choices().length > 1}>
          <RadioCards
            label={d.image}
            value={chosen() ? choiceKey(chosen() as ImageChoice) : ""}
            onChange={(value) => {
              setPicked(value);
            }}
            disabled={busy()}
            options={choices().map((choice) => ({ value: choiceKey(choice), ...label(choice) }))}
          />
        </Show>
        <Show when={release()}>
          <div class="flex flex-col gap-1.5">
            <Segmented
              label={d.source}
              value={source()}
              onChange={setSource}
              disabled={busy()}
              class="w-fit"
              options={(["asu", "official"] as const).map((value) => ({
                value,
                label: d.sources[value] ?? value,
              }))}
            />
            <p class="text-fg-subtle text-xs">{d.sourceHint[source()]}</p>
          </div>
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
        <Show when={githubRelease()}>
          {(current) => (
            <a
              href={current().notes}
              target="_blank"
              rel="noopener noreferrer"
              class="inline-flex w-fit items-center gap-1.5 text-accent text-sm hover:underline"
            >
              {t.notes} · {current().name}
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
          {(found) => (
            <>
              <PlanView plan={found()} />
            </>
          )}
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
        <InstallStatus
          install={install}
          name={props.name}
          official={plan()?.source === "official"}
        />
        <DialogFooter>
          <Button size="sm" onClick={() => props.onOpenChange(false)}>
            {d.close}
          </Button>
          <Show when={!buildable() || build()?.state === "done"}>
            <InstallButton install={install} />
          </Show>
          <Show when={buildable() && (build() === undefined || build()?.state === "failed")}>
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
        </DialogFooter>
      </div>
    </Dialog>
  );
}
