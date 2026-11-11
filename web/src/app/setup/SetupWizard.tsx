import { useNavigate } from "@solidjs/router";
import { useQuery } from "@tanstack/solid-query";
import { createMemo, createSignal, For, Match, Show, Switch, untrack, useContext } from "solid-js";
import type { Band } from "../../api/gen/Band";
import type { NodeSummary } from "../../api/gen/NodeSummary";
import type { Preset } from "../../api/gen/Preset";
import type { SiteView } from "../../api/gen/SiteView";
import { createWanForm } from "../../features/internet/wanForm";
import { ApiError, api } from "../../lib/api";
import { countryOfLocale } from "../../lib/countries";
import { createJobsView } from "../../lib/jobs";
import { LiveContext } from "../../lib/live";
import { SessionContext } from "../../lib/session";
import { saveSite } from "../../lib/site";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { RequestError } from "../../ui/JobOutcome";
import { Wordmark } from "../../ui/Wordmark";
import { AddressStep } from "./AddressStep";
import { InternetStep } from "./InternetStep";
import {
  type AddressDraft,
  addressDraftOf,
  checkAddresses,
  checkWifi,
  mainNetwork,
  moves,
  setupSecrets,
  type WifiDraft,
  withSetup,
  wlanBands,
} from "./plan";
import { ReviewStep } from "./ReviewStep";
import { RouterStep } from "./RouterStep";
import { bandNames, WifiStep } from "./WifiStep";

const t = strings.setup;

type Step = "router" | "internet" | "addresses" | "wifi" | "review";
const STEPS: readonly Step[] = ["router", "internet", "addresses", "wifi", "review"];

/**
 * `/setup`, the first-run wizard (adoption.md, Path E; D-080, D-082): a full page outside the
 * shell, opened after the admin account is created and from the dashboard while no gateway is
 * adopted. Router → Internet → Addresses → Wi-Fi → Review, saved as one revision and applied
 * live. Leaving it at any step is safe: nothing changes on the router before the apply.
 */
export function SetupWizard() {
  const live = useContext(LiveContext);
  const session = useContext(SessionContext);
  const navigate = useNavigate();
  const site = useQuery(() => ({
    queryKey: ["site", "setup"],
    queryFn: ({ signal }) => api.site.get(signal),
    staleTime: Number.POSITIVE_INFINITY,
  }));
  const presets = useQuery(() => ({
    queryKey: ["isp-presets"],
    queryFn: ({ signal }) => api.wan.presets(signal),
    staleTime: Number.POSITIVE_INFINITY,
  }));
  const [done, setDone] = createSignal(false);
  const loaded = () => {
    const view = site.data;
    const list = presets.data;
    return view && list ? { view, list } : undefined;
  };

  return (
    <main class="flex min-h-dvh flex-col items-center bg-canvas px-4 py-8 sm:py-12">
      <div class="flex w-full max-w-3xl flex-col gap-6">
        <header class="flex flex-wrap items-center justify-between gap-3">
          <Wordmark />
          <Show when={!done()}>
            <Button variant="ghost" size="sm" onClick={() => navigate("/")}>
              {t.skip}
            </Button>
          </Show>
        </header>
        <div>
          <h1 class="font-semibold text-2xl tracking-tight">{t.title}</h1>
          <p class="mt-1.5 max-w-2xl text-fg-muted text-sm leading-relaxed">{t.description}</p>
        </div>
        <Switch>
          <Match when={session.user.role !== "admin"}>
            <Card class="p-6">
              <p class="text-fg-muted text-sm">{t.adminsOnly}</p>
            </Card>
          </Match>
          <Match when={site.isError || presets.isError}>
            <Card class="flex flex-col items-start gap-3 p-6">
              <RequestError text={t.loadFailed} />
              <Button
                variant="secondary"
                onClick={() => {
                  void site.refetch();
                  void presets.refetch();
                }}
              >
                {t.retry}
              </Button>
            </Card>
          </Match>
          <Match when={live.fleet.ready && loaded()}>
            {(data) => (
              <Steps site={data().view} presets={data().list} onDone={() => setDone(true)} />
            )}
          </Match>
          <Match when={true}>
            <Card class="p-6">
              <span class="block h-24 animate-pulse rounded-control bg-raised" aria-hidden="true" />
            </Card>
          </Match>
        </Switch>
      </div>
    </main>
  );
}

/** The browser's time zone, for a site that still has the default. */
function browserZone(current: string): string | undefined {
  if (current !== "UTC") return undefined;
  const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  return zone && zone !== "UTC" ? zone : undefined;
}

/** The wizard's steps, mounted once the site and the presets are read: its drafts start there. */
function Steps(props: { site: SiteView; presets: readonly Preset[]; onDone: () => void }) {
  const live = useContext(LiveContext);
  const navigate = useNavigate();
  const [jobs, addJobs] = createJobsView(live);
  const config = untrack(() => props.site.config);
  const storedPassword = untrack(() =>
    props.site.secrets.some((s) => s.path === "wan/password" && s.set),
  );

  const gateway = (): NodeSummary | undefined =>
    live.fleet.nodes.find((node) => node.role === "gateway");
  const [step, setStep] = createSignal<Step>("router");
  const form = createWanForm({
    current: config.wan,
    storedPassword,
    presets: () => props.presets,
    jobs,
    addJobs,
  });
  const [addresses, setAddresses] = createSignal<AddressDraft>(addressDraftOf(config));
  const [wifi, setWifi] = createSignal<WifiDraft>({
    enabled: true,
    ssid: "",
    passphrase: "",
    country: config.site.country ?? countryOfLocale(navigator.language) ?? "",
  });
  const [zone, setZone] = createSignal(browserZone(config.site.timezone));
  const [picking, setPicking] = createSignal(config.wan.preset === null);

  // The router's radios, read until its first facts are in.
  const detail = useQuery(() => ({
    queryKey: ["setup-gateway", gateway()?.id],
    queryFn: ({ signal }) => api.nodes.get(gateway()?.id ?? 0, signal),
    enabled: gateway() !== undefined,
    refetchInterval: (query) => (query.state.data?.facts ? false : 2000),
  }));
  const radios = createMemo<Band[]>(
    () => detail.data?.facts?.radios.map((radio) => radio.band) ?? [],
  );
  const bands = createMemo(() => wlanBands(radios()));
  const chosenWifi = () => (bands().length > 0 && wifi().enabled ? wifi() : undefined);

  const rows = (): [string, string][] => {
    const lan = mainNetwork(addresses());
    const chosen = chosenWifi();
    const router = gateway();
    const out: [string, string][] = [
      [t.review.router, [router?.name, router?.model].filter(Boolean).join(" · ")],
      [t.review.internet, form.label()],
      [t.review.addresses, t.review.addressesOf(lan.subnet, lan.address.split("/")[0] ?? "")],
      [
        t.review.wifi,
        chosen ? t.review.wifiOn(chosen.ssid, bandNames(bands()), chosen.country) : t.review.noWifi,
      ],
    ];
    const tz = zone();
    if (tz) out.push([t.review.timezone, t.review.fromBrowser(tz)]);
    return out;
  };

  /** One revision with every choice; a time zone the controller doesn't know is left out. */
  const save = async (): Promise<SiteView> => {
    const router = gateway();
    const wan = form.outcome(true);
    if (!router || !wan) {
      setStep(router ? "internet" : "router");
      throw new Error(strings.internet.errors.fix);
    }
    const once = (timezone: string | undefined) =>
      saveSite(
        (latest) =>
          withSetup(latest, {
            gateway: router.id,
            wan,
            addresses: addresses(),
            wifi: chosenWifi(),
            bands: bands(),
            timezone,
          }),
        t.review.message,
        setupSecrets(form.secrets(), chosenWifi()),
      );
    try {
      return await once(zone());
    } catch (err) {
      const zoneRefused =
        err instanceof ApiError && err.issues.some((issue) => issue.path === "site/timezone");
      if (!zoneRefused || zone() === undefined) throw err;
      setZone(undefined);
      return once(undefined);
    }
  };

  const index = () => STEPS.indexOf(step());

  return (
    <>
      <ol class="flex flex-wrap gap-x-2 gap-y-1.5 text-sm" aria-label={t.title}>
        <For each={STEPS}>
          {(s, i) => (
            <li
              class={[
                "flex items-center gap-2",
                s === step() ? "font-medium text-fg" : "text-fg-subtle",
              ]}
              aria-current={s === step() ? "step" : undefined}
            >
              <Show when={i() > 0}>
                <span class="h-px w-4 bg-edge sm:w-8" aria-hidden="true" />
              </Show>
              <span
                class={[
                  "grid size-6 place-items-center rounded-full border text-xs tabular-nums",
                  i() < index()
                    ? "border-accent bg-accent/15 text-accent"
                    : s === step()
                      ? "border-accent text-fg"
                      : "border-edge",
                ]}
                aria-hidden="true"
              >
                {i() + 1}
              </span>
              {t.steps[s]}
            </li>
          )}
        </For>
      </ol>
      <Card class="p-5 sm:p-7">
        <Switch>
          <Match when={step() === "router"}>
            <RouterStep
              jobs={jobs}
              addJobs={addJobs}
              gateway={gateway()}
              onNext={() => setStep("internet")}
              onImport={() => navigate("/settings/import")}
            />
          </Match>
          <Match when={step() === "internet"}>
            <InternetStep
              form={form}
              presets={props.presets}
              country={config.site.country ?? (wifi().country || null)}
              storedPassword={storedPassword}
              picking={picking()}
              onPicking={setPicking}
              onBack={() => setStep("router")}
              onNext={() => setStep("addresses")}
            />
          </Match>
          <Match when={step() === "addresses"}>
            <AddressStep
              draft={addresses()}
              errors={checkAddresses(addresses())}
              host={gateway()?.host ?? ""}
              domain={config.site.domain}
              onChange={(next) => setAddresses((now) => ({ ...now, ...next }))}
              onBack={() => setStep("internet")}
              onNext={() => setStep("wifi")}
            />
          </Match>
          <Match when={step() === "wifi"}>
            <WifiStep
              draft={wifi()}
              errors={bands().length > 0 ? checkWifi(wifi()) : {}}
              bands={bands()}
              sixGhz={radios().includes("6g")}
              onChange={(next) => setWifi((now) => ({ ...now, ...next }))}
              onBack={() => setStep("addresses")}
              onNext={() => setStep("review")}
            />
          </Match>
          <Match when={step() === "review" && gateway()}>
            {(router) => (
              <ReviewStep
                gateway={router()}
                rows={rows()}
                renumbers={moves(router().host, addresses())}
                save={save}
                jobs={jobs}
                addJobs={addJobs}
                onBack={() => setStep("wifi")}
                onInternet={() => setStep("internet")}
                onDone={props.onDone}
                onLeave={(to) => navigate(to)}
              />
            )}
          </Match>
        </Switch>
      </Card>
    </>
  );
}
