import { TriangleAlert } from "lucide";
import { For, Show } from "solid-js";
import type { UpgradePlan } from "../../api/gen/UpgradePlan";
import { tightTmp } from "../../lib/firmware";
import { formatBytes, formatTime } from "../../lib/format";
import { strings } from "../../lib/strings";
import { Icon } from "../../ui/Icon";
import { ImageRow } from "../../ui/ImageBuild";

const d = strings.firmware.dialog;

/**
 * What an upgrade holds against the node (D-121, D-122): with ASU, what the image keeps,
 * renames, adds and drops (dropped packages as a warning); with an official image, the same
 * packages installed after the upgrade, and the image's files; with an upload, the image; with a
 * GitHub release, its images, flashed as they are.
 */
export function PlanView(props: { plan: UpgradePlan }) {
  const official = () => props.plan.source === "official";
  const free = () =>
    props.plan.tmp_free_bytes === null ? null : formatBytes(props.plan.tmp_free_bytes);
  const keptLabel = () =>
    official() ? d.installed(props.plan.kept.length) : d.kept(props.plan.kept.length);
  return (
    <section class="flex flex-col gap-3" aria-label={keptLabel()}>
      <Show
        when={props.plan.upload}
        fallback={
          <>
            <Show when={props.plan.dropped.length > 0}>
              <div
                role="alert"
                class="rounded-control border border-warn/40 bg-warn/10 px-3 py-2.5"
              >
                <p class="flex items-center gap-2 font-medium text-fg text-sm">
                  <Icon icon={TriangleAlert} class="size-4 shrink-0 text-warn" />
                  {official() ? d.notInstalled(props.plan.release) : d.dropped(props.plan.release)}
                </p>
                <Names names={props.plan.dropped} />
                <p class="mt-1.5 text-fg-muted text-xs">{d.droppedHint}</p>
              </div>
            </Show>
            <Group label={keptLabel()} names={props.plan.kept} />
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
          </>
        }
      >
        {(upload) => (
          <div class="rounded-control border border-edge bg-canvas px-3 py-2.5 text-sm">
            <p class="font-medium text-fg">{upload().name}</p>
            <p class="text-fg-muted text-xs">
              {[upload().distribution, upload().version, upload().revision, upload().target]
                .filter((part) => part !== null)
                .join(" · ")}
            </p>
            <p class="mt-1 break-all font-mono text-fg-subtle text-xs">
              {d.sha256} {upload().sha256}
            </p>
            <p class="mt-1.5 text-fg-muted text-xs">{d.uploadNote}</p>
          </div>
        )}
      </Show>
      <Show when={props.plan.source === "github"}>
        <p class="text-fg-muted text-xs">{d.githubNote}</p>
      </Show>
      <Show when={props.plan.images.length > 0}>
        <ul
          class="flex flex-col gap-2"
          aria-label={
            props.plan.source === "github" ? d.githubFiles(props.plan.repo ?? "GitHub") : d.files
          }
        >
          <For each={props.plan.images}>
            {(image) => <ImageRow image={image} size={formatBytes(image.size_bytes)} />}
          </For>
        </ul>
      </Show>
      <Show when={props.plan.image_bytes !== null}>
        <p class="text-fg-subtle text-xs">
          {props.plan.source === "asu"
            ? d.size(formatBytes(props.plan.image_bytes), free())
            : d.exactSize(formatBytes(props.plan.image_bytes), free())}
        </p>
      </Show>
      <Show when={tightTmp(props.plan)}>
        <p class="flex items-center gap-2 text-sm text-warn">
          <Icon icon={TriangleAlert} class="size-4 shrink-0" />
          {d.tight}
        </p>
      </Show>
      <Show when={props.plan.packages_at}>
        {(at) => <p class="text-fg-subtle text-xs">{d.read(formatTime(at()))}</p>}
      </Show>
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
