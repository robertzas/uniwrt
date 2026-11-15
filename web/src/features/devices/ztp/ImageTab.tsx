import { ExternalLink, Hammer, ShieldCheck } from "lucide";
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  Match,
  onSettled,
  Show,
  Switch,
} from "solid-js";
import type { FirmwareProfile } from "../../../api/gen/FirmwareProfile";
import type { ImageBuild } from "../../../api/gen/ImageBuild";
import { api, errorText } from "../../../lib/api";
import { followBuild } from "../../../lib/firmware";
import { formatBytes } from "../../../lib/format";
import { strings } from "../../../lib/strings";
import { Button } from "../../../ui/Button";
import { FormField, INPUT, SELECT } from "../../../ui/FormField";
import { Icon } from "../../../ui/Icon";
import { BuildFailed, BuildRunning, ImageRow } from "../../../ui/ImageBuild";
import { JobSuccessNote, RequestError } from "../../../ui/JobOutcome";
import { matchProfiles, packageChanges } from "./image";
import { RolePicker } from "./ZtpDialogs";

const t = strings.ztp.setup.image;
const role = strings.devices.role;
const roles = [
  { value: "ap", label: role.ap },
  { value: "bridge", label: role.bridge },
  { value: "gateway", label: role.gateway },
] as const;
type Role = (typeof roles)[number]["value"];

/**
 * Builds a zero-touch image through ASU (adoption.md, Path B): a release, a model from its
 * catalog and a role, then the build followed until its images can be downloaded.
 */
export function ImageTab(props: { open: boolean }) {
  const [releases, setReleases] = createSignal<string[]>([]);
  const [release, setRelease] = createSignal<string>();
  const [profiles, setProfiles] = createSignal<FirmwareProfile[]>();
  const [query, setQuery] = createSignal("");
  const [picked, setPicked] = createSignal<FirmwareProfile>();
  const [buildRole, setBuildRole] = createSignal<Role>("ap");
  const [packages, setPackages] = createSignal<string[]>([]);
  const [build, setBuild] = createSignal<ImageBuild>();
  const [error, setError] = createSignal<string>();
  const [pending, setPending] = createSignal(false);

  onSettled(() => {
    const controller = new AbortController();
    api.ztp
      .releases(controller.signal)
      .then((found) => {
        setReleases(found.releases);
        setRelease(found.releases[0]);
      })
      .catch((err) => {
        if (!controller.signal.aborted) setError(errorText(err));
      });
    return () => controller.abort();
  });
  // The release's models, read again when another release is chosen.
  createEffect(release, (chosen) => {
    if (!chosen) return;
    const controller = new AbortController();
    setProfiles(undefined);
    api.ztp
      .profiles(chosen, controller.signal)
      .then(setProfiles)
      .catch((err) => {
        if (!controller.signal.aborted) setError(errorText(err));
      });
    return () => controller.abort();
  });
  followBuild({
    build,
    active: () => props.open,
    fetch: (hash, signal) => api.ztp.builds.get(hash, signal),
    onBuild: setBuild,
    onError: setError,
  });

  const matches = createMemo(() => matchProfiles(profiles() ?? [], query()));
  const start = async () => {
    const device = picked();
    const chosen = release();
    if (!device || !chosen) return;
    setPending(true);
    setError(undefined);
    try {
      const started = await api.ztp.builds.create({
        release: chosen,
        target: device.target,
        profile: device.id,
        role: buildRole(),
      });
      setPackages(started.packages);
      setBuild(started.build);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setPending(false);
    }
  };

  return (
    <div class="flex flex-col gap-4">
      <p class="text-fg-muted text-sm">{t.intro}</p>
      <RequestError text={error()} />
      <Show
        when={build()}
        fallback={
          <form
            class="flex flex-col gap-4"
            onSubmit={(event) => {
              event.preventDefault();
              void start();
            }}
          >
            <FormField label={t.release} for="ztp-release" class="max-w-56">
              <select
                id="ztp-release"
                class={SELECT}
                value={release() ?? ""}
                disabled={releases().length === 0}
                onChange={(event) => {
                  setRelease(event.currentTarget.value);
                  setPicked(undefined);
                }}
              >
                <For each={releases()}>
                  {(name, index) => (
                    <option value={name}>{index() === 0 ? `${name} (${t.latest})` : name}</option>
                  )}
                </For>
              </select>
            </FormField>
            <Show
              when={picked()}
              fallback={
                <ModelSearch
                  query={query()}
                  onQuery={setQuery}
                  loading={profiles() === undefined}
                  total={profiles()?.length ?? 0}
                  shown={matches().shown}
                  more={matches().more}
                  onPick={setPicked}
                />
              }
            >
              {(device) => (
                <div>
                  <p class="mb-1.5 font-medium text-fg text-sm">{t.model}</p>
                  <div class="flex items-center gap-3 rounded-control border border-edge bg-canvas px-3 py-2">
                    <div class="min-w-0 flex-1">
                      <p class="truncate text-fg text-sm" data-testid="ztp-model">
                        {device().titles.join(" · ")}
                      </p>
                      <p class="truncate font-mono text-fg-subtle text-xs">
                        {device().target} · {device().id}
                      </p>
                    </div>
                    <Button size="sm" variant="ghost" onClick={() => setPicked(undefined)}>
                      {t.change}
                    </Button>
                  </div>
                </div>
              )}
            </Show>
            <RolePicker
              label={strings.ztp.setup.role}
              value={buildRole()}
              options={roles}
              onChange={setBuildRole}
            />
            <p class="flex gap-1.5 text-fg-subtle text-xs">
              <Icon icon={ShieldCheck} class="mt-0.5 size-3.5 shrink-0 text-ok" />
              {t.noSecrets}
            </p>
            <div>
              <Button
                type="submit"
                variant="primary"
                icon={Hammer}
                disabled={pending() || !picked() || !release()}
              >
                {pending() ? t.starting : t.build}
              </Button>
            </div>
          </form>
        }
      >
        {(current) => (
          <Progress
            build={current()}
            packages={packages()}
            model={picked()?.titles[0] ?? ""}
            onAgain={() => void start()}
            onAnother={() => {
              setBuild(undefined);
              setError(undefined);
            }}
          />
        )}
      </Show>
    </div>
  );
}

function ModelSearch(props: {
  query: string;
  onQuery: (query: string) => void;
  loading: boolean;
  total: number;
  shown: FirmwareProfile[];
  more: number;
  onPick: (profile: FirmwareProfile) => void;
}) {
  return (
    <FormField
      label={t.model}
      for="ztp-model-search"
      hint={props.loading ? t.loadingModels : t.searchHint(props.total)}
    >
      <input
        id="ztp-model-search"
        type="search"
        class={[INPUT, "border-edge"]}
        placeholder={t.search}
        value={props.query}
        disabled={props.loading}
        autocomplete="off"
        onInput={(event) => props.onQuery(event.currentTarget.value)}
      />
      <Show when={props.query.trim() && !props.loading}>
        <Show
          when={props.shown.length > 0}
          fallback={<p class="text-fg-muted text-xs">{t.noMatch}</p>}
        >
          <ul
            class="flex flex-col divide-y divide-edge overflow-hidden rounded-control border border-edge"
            aria-label={t.model}
          >
            <For each={props.shown}>
              {(profile) => (
                <li>
                  <button
                    type="button"
                    class="flex w-full items-baseline gap-2 bg-canvas px-3 py-1.5 text-left text-sm hover:bg-raised focus-visible:outline-2 focus-visible:outline-accent"
                    onClick={() => props.onPick(profile)}
                  >
                    <span class="min-w-0 flex-1 truncate text-fg">
                      {profile.titles.join(" · ")}
                    </span>
                    <span class="shrink-0 font-mono text-fg-subtle text-xs">{profile.target}</span>
                  </button>
                </li>
              )}
            </For>
          </ul>
          <Show when={props.more > 0}>
            <p class="text-fg-subtle text-xs">{t.more(props.more)}</p>
          </Show>
        </Show>
      </Show>
    </FormField>
  );
}

function Progress(props: {
  build: ImageBuild;
  packages: string[];
  model: string;
  onAgain: () => void;
  onAnother: () => void;
}) {
  const changes = () => packageChanges(props.packages);
  return (
    <div class="flex flex-col gap-4">
      <Switch>
        <Match when={props.build.state === "queued" || props.build.state === "building"}>
          <BuildRunning build={props.build} />
        </Match>
        <Match when={props.build.state === "done"}>
          <JobSuccessNote text={t.state.done} />
          <ul class="flex flex-col gap-2" aria-label={t.state.done}>
            <For each={props.build.images}>
              {(image) => <ImageRow image={image} size={formatBytes(image.size_bytes)} />}
            </For>
          </ul>
          <a
            href={`https://openwrt.org/start?do=search&q=${encodeURIComponent(props.model)}`}
            target="_blank"
            rel="noopener noreferrer"
            class="inline-flex w-fit items-center gap-1.5 text-accent text-sm hover:underline"
          >
            {t.flashing}
            <Icon icon={ExternalLink} class="size-3.5" />
          </a>
        </Match>
        <Match when={props.build.state === "failed"}>
          <BuildFailed build={props.build} />
        </Match>
      </Switch>
      <Show when={changes().added.length > 0}>
        <p class="text-fg-subtle text-xs">{t.packages(changes().added, changes().removed)}</p>
      </Show>
      <div class="flex gap-2">
        <Show when={props.build.state === "failed"}>
          <Button size="sm" variant="primary" onClick={props.onAgain}>
            {t.tryAgain}
          </Button>
        </Show>
        <Button size="sm" onClick={props.onAnother}>
          {t.another}
        </Button>
      </div>
    </div>
  );
}
