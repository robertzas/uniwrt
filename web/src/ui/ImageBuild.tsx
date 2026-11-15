import { Download, LoaderCircle } from "lucide";
import { Show } from "solid-js";
import { strings } from "../lib/strings";
import { Icon } from "./Icon";

const t = strings.ztp.setup.image;

/** What these show of an image build at ASU (the API's `ImageBuild`). */
interface ImageBuild {
  state: string;
  queue_position: number | null;
  step: string | null;
  error: string | null;
}

/** What a row shows of a built image (the API's `FirmwareImage`). */
interface FirmwareImage {
  name: string;
  kind: string;
  sha256: string;
  url: string;
}

/** A build while ASU queues or builds it: where it stands, and that it takes a while. */
export function BuildRunning(props: { build: ImageBuild }) {
  const step = () => {
    const name = props.build.step ?? "init";
    return t.step[name] ?? name.replaceAll("_", " ");
  };
  return (
    <div class="flex flex-col gap-1">
      <p role="status" class="flex items-center gap-2 text-fg text-sm">
        <Icon icon={LoaderCircle} class="size-4 animate-spin text-accent" />
        {props.build.state === "queued"
          ? t.state.queued(props.build.queue_position)
          : t.state.building(step())}
      </p>
      <p class="text-fg-subtle text-xs">{t.wait}</p>
    </div>
  );
}

/** A failed build, with the end of the image builder's output. */
export function BuildFailed(props: { build: ImageBuild }) {
  return (
    <div role="alert" class="flex flex-col gap-2">
      <p class="font-medium text-crit text-sm">{t.state.failed}</p>
      <Show when={props.build.error}>
        {(why) => (
          <pre class="max-h-48 overflow-auto whitespace-pre-wrap break-words rounded-control border border-edge bg-canvas px-3 py-2 font-mono text-fg-muted text-xs">
            {why()}
          </pre>
        )}
      </Show>
    </div>
  );
}

/** A built image: what it is for, its size (formatted by the caller), its download and its SHA-256. */
export function ImageRow(props: { image: FirmwareImage; size: string }) {
  const kind = () => t.kind[props.image.kind] ?? props.image.kind;
  return (
    <li class="flex flex-col gap-1.5 rounded-control border border-edge bg-canvas px-3 py-2.5">
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
        <div class="min-w-0 flex-1">
          <p class="font-medium text-fg text-sm">{kind()}</p>
          <Show when={t.kindHint[props.image.kind]}>
            {(hint) => <p class="text-fg-subtle text-xs">{hint()}</p>}
          </Show>
        </div>
        <span class="text-fg-muted text-xs tabular-nums">{props.size}</span>
        <a
          href={props.image.url}
          rel="noopener noreferrer"
          aria-label={`${t.download} ${kind()}`}
          class="inline-flex h-7 items-center gap-1.5 rounded-control bg-accent-fill px-2.5 text-on-accent text-xs hover:brightness-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
        >
          <Icon icon={Download} class="size-3.5" />
          {t.download}
        </a>
      </div>
      <p class="truncate font-mono text-fg-subtle text-xs" title={props.image.name}>
        {props.image.name}
      </p>
      <p class="break-all font-mono text-fg-subtle text-xs">
        {t.sha256} {props.image.sha256}
      </p>
    </li>
  );
}
