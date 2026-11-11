import { createMemo, createSignal } from "solid-js";
import type { Job } from "../../api/gen/Job";
import type { JobId } from "../../api/gen/JobId";
import type { Preset } from "../../api/gen/Preset";
import type { WanConfig } from "../../api/gen/WanConfig";
import { ApiError, api, errorText } from "../../lib/api";
import type { JobsState } from "../../lib/jobs";
import { strings } from "../../lib/strings";
import { check, comboError, type DraftErrors, errorsOfIssues } from "./check";
import {
  type Draft,
  draftOf,
  type Field,
  fallbackOf,
  fieldsOf,
  promptedFields,
  wanOf,
  withPreset,
} from "./draft";
import { ipv6Result } from "./summary";

/**
 * The WAN form's state (wan.md, Wizard flow), which the WAN wizard and the first-run wizard
 * share: the draft with its preset, the checks, the test on the gateway and what it found.
 */
export function createWanForm(args: {
  /** The site's WAN, which the form starts from. */
  current: WanConfig;
  /** The vault holds a PPPoE password already. */
  storedPassword: boolean;
  presets: () => readonly Preset[];
  jobs: JobsState;
  addJobs: (jobs: readonly Job[]) => void;
}) {
  const [draft, setDraft] = createSignal<Draft>(draftOf(args.current));
  const [errors, setErrors] = createSignal<DraftErrors>({});
  const [failure, setFailure] = createSignal<string>();
  const [advanced, setAdvanced] = createSignal(args.current.preset === null);
  const [jobId, setJobId] = createSignal<JobId>();
  const [ipv6Off, setIpv6Off] = createSignal(false);
  const [busy, setBusy] = createSignal(false);

  const preset = createMemo(() => args.presets().find((p) => p.id === draft().preset));
  const job = () => {
    const id = jobId();
    return id === undefined ? undefined : args.jobs.byId[id];
  };
  const prompted = createMemo(() => {
    const p = preset();
    return p ? promptedFields(p) : [];
  });
  const rest = createMemo<Field[]>(() =>
    fieldsOf(draft()).filter((field) => !prompted().includes(field)),
  );

  const change = (next: Partial<Draft>) => {
    setDraft((now) => ({ ...now, ...next }));
    setErrors({});
    setFailure(undefined);
    // What was tested is no longer what the form says.
    setJobId(undefined);
  };
  const pick = (chosen: Preset) => {
    setDraft((now) => withPreset(now, chosen));
    setErrors({});
    setFailure(undefined);
    setAdvanced(false);
    setJobId(undefined);
  };
  /** The form's WAN, or nothing after showing what is wrong. */
  const valid = (): WanConfig | undefined => {
    const found = check(draft(), args.storedPassword);
    const combo = comboError(draft());
    setErrors(found);
    if (Object.keys(found).length > 0 || combo) {
      if (Object.keys(found).some((field) => !prompted().includes(field as Field))) {
        setAdvanced(true);
      }
      if (combo) setAdvanced(true);
      setFailure(combo ?? strings.internet.errors.fix);
      return undefined;
    }
    return wanOf(draft(), args.current);
  };
  /** Shows why a request failed; `true` when the server named fields of the form. */
  const refused = (err: unknown): boolean => {
    if (err instanceof ApiError && err.issues.length > 0) {
      const { errors: found, other } = errorsOfIssues(err.issues);
      setErrors(found);
      setAdvanced(true);
      setFailure(other[0] ?? strings.internet.errors.fix);
      return true;
    }
    setFailure(errorText(err));
    return false;
  };

  /** Starts the trial on the gateway; `false` when the form is wrong or the request failed. */
  const test = async (onStart?: () => void): Promise<boolean> => {
    const wan = valid();
    if (!wan) return false;
    setBusy(true);
    setIpv6Off(false);
    setJobId(undefined);
    onStart?.();
    try {
      const started = await api.wan.test({
        wan,
        password: draft().password === "" ? undefined : draft().password,
        fallback: fallbackOf(preset()),
      });
      args.addJobs([started]);
      setJobId(started.id);
      return true;
    } catch (err) {
      refused(err);
      return false;
    } finally {
      setBusy(false);
    }
  };

  /**
   * The WAN to save, or nothing after showing what is wrong. With `tested`, a test that
   * succeeded decides IPv6: a 6rd fallback that worked is kept, and IPv6 goes off when it
   * failed alone and the user chose so.
   */
  const outcome = (tested: boolean): WanConfig | undefined => {
    const wan = valid();
    if (!wan) return undefined;
    const finished = job();
    const fallback = fallbackOf(preset());
    if (tested && finished?.state === "succeeded") {
      if (ipv6Result(finished) === "sixrd" && fallback) wan.ipv6 = fallback;
      if (ipv6Result(finished) === "failed" && ipv6Off()) wan.ipv6 = { mode: "off" };
    }
    return wan;
  };

  return {
    draft,
    errors,
    failure,
    setFailure,
    advanced,
    setAdvanced,
    preset,
    prompted,
    rest,
    job,
    ipv6Off,
    setIpv6Off,
    busy,
    setBusy,
    change,
    pick,
    valid,
    refused,
    test,
    outcome,
    /** The secrets a save carries: a new PPPoE password, if one was typed. */
    secrets: () => (draft().password === "" ? undefined : { "wan/password": draft().password }),
    /** What the connection is called: the preset's name, or "Custom". */
    label: () => {
      const p = preset();
      return p ? `${p.name} · ${p.variant}` : strings.internet.custom;
    },
    running: () => busy() || job()?.state === "running",
  };
}

export type WanForm = ReturnType<typeof createWanForm>;
