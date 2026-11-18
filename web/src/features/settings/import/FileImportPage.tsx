import { Download, KeyRound, Lock, RotateCw } from "lucide";
import { createEffect, createMemo, createSignal, Show, useContext } from "solid-js";
import type { SiteImportAccept } from "../../../api/gen/SiteImportAccept";
import type { SiteImportPreview } from "../../../api/gen/SiteImportPreview";
import { ApiError, api, errorText } from "../../../lib/api";
import { formatDateTime } from "../../../lib/format";
import { LiveContext } from "../../../lib/live";
import { SessionContext } from "../../../lib/session";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { EmptyState } from "../../../ui/EmptyState";
import { RequestError } from "../../../ui/JobOutcome";
import { TextField } from "../../../ui/TextField";
import { showToast } from "../../../ui/Toast";
import { ChangeList } from "../revisions/ChangeList";
import { FileNodes } from "./FileNodes";
import { FilePick } from "./FilePick";
import { FileSecrets } from "./FileSecrets";
import { filledSecrets, missingPaths, type PickedFile, sortFileIssues } from "./file";
import { ImportDone, Issues } from "./parts";

const t = strings.settings.fileImport;

/**
 * `/settings/import/file`, Settings › Import a file (lifecycle.md, Backups: Site import; D-130):
 * a site file exported here or elsewhere, opened with its passphrase when it holds secrets, then
 * the preview the server holds (changes, the node mapping by MAC, each secret's source with
 * fields for the missing ones); accepting saves it as the next revision (kind `file`), then
 * offers its review. Admins only.
 */
export function FileImportPage() {
  const session = useContext(SessionContext);
  createEffect(
    () => t.title,
    (text) => {
      document.title = `${text} · ${strings.appName}`;
    },
  );
  return (
    <div class="mx-auto flex max-w-5xl flex-col gap-4">
      <header>
        <nav aria-label={strings.nav.settings.label} class="text-fg-subtle text-xs">
          <a href="/settings?tab=revisions" class="hover:text-fg hover:underline">
            {strings.nav.settings.label}
          </a>
          <span aria-hidden="true">{" › "}</span>
          <span aria-current="page">{t.crumb}</span>
        </nav>
        <h1 class="mt-1 font-semibold text-2xl tracking-tight">{t.title}</h1>
        <p class="mt-1 text-fg-muted text-sm">{t.description}</p>
      </header>
      <Show
        when={session.user.role === "admin"}
        fallback={
          <Card>
            <EmptyState icon={Lock} {...t.adminOnly} />
          </Card>
        }
      >
        <Flow />
      </Show>
    </div>
  );
}

function Flow() {
  const live = useContext(LiveContext);
  const [picked, setPicked] = createSignal<PickedFile>();
  const [passphrase, setPassphrase] = createSignal("");
  const [preview, setPreview] = createSignal<SiteImportPreview>();
  const [error, setError] = createSignal<string>();
  const [loading, setLoading] = createSignal(false);
  const [values, setValues] = createSignal<Record<string, string>>({});
  const [message, setMessage] = createSignal("");
  const [accepting, setAccepting] = createSignal(false);
  /** The revision the accepted import saved; the base when it saved nothing. */
  const [saved, setSaved] = createSignal<{ id: number; base: number }>();
  const nodeName = (id: number) => live.fleet.nodes.find((n) => n.id === id)?.name;

  /** Previews the picked file, keeping a preview shown until the new one arrives. */
  const load = async () => {
    const file = picked();
    if (!file) return;
    setLoading(true);
    setError(undefined);
    try {
      const pass = passphrase();
      const next = await api.siteFile.preview(
        pass ? { file: file.value, passphrase: pass } : { file: file.value },
      );
      if (!Array.isArray(next?.secrets)) throw new Error("not a site import preview");
      setPreview(next);
      setSaved(undefined);
    } catch (err) {
      setError(errorText(err) ?? t.failed);
    } finally {
      setLoading(false);
    }
  };
  /** Back to the file, to give the passphrase or choose another. */
  const back = () => {
    setPreview(undefined);
    setError(undefined);
    setSaved(undefined);
  };

  const sorted = createMemo(() => {
    const current = preview();
    return sortFileIssues(current?.issues ?? [], current?.secrets ?? [], values());
  });
  const missing = createMemo(() => missingPaths(preview()?.secrets ?? []));
  const filled = createMemo(() => Object.keys(filledSecrets(preview()?.secrets ?? [], values())));
  /** Errors a value typed here can't clear. */
  const hard = createMemo(() => sorted().blocking.filter((i) => !missing().has(i.path)));
  const changes = () => preview()?.changes.length ?? 0;
  const stale = () => {
    const base = preview()?.base;
    const head = live.config.head?.id;
    return base !== undefined && head !== undefined && head !== base && saved() === undefined;
  };
  const canAccept = () =>
    !stale() &&
    !loading() &&
    !accepting() &&
    sorted().blocking.length === 0 &&
    (changes() > 0 || filled().length > 0);

  const accept = async () => {
    const current = preview();
    if (!current) return;
    setAccepting(true);
    try {
      const body: SiteImportAccept = { id: current.id };
      const text = message().trim();
      if (text !== "") body.message = text;
      const secrets = filledSecrets(current.secrets, values());
      if (Object.keys(secrets).length > 0) body.secrets = secrets;
      const view = await api.siteFile.accept(body);
      setSaved({ id: view.revision.id, base: current.base });
      setMessage("");
      setValues({});
    } catch (err) {
      showToast({ title: t.acceptFailed, description: errorText(err), tone: "crit" });
      if (err instanceof ApiError && err.status === 422 && err.issues.length > 0) {
        // Still held: its issues now include the values refused.
        setPreview({ ...current, issues: [...err.issues] });
      } else if (err instanceof ApiError && (err.status === 404 || err.status === 409)) {
        // Expired, or another revision was saved since: only a new preview can be accepted.
        load();
      }
    } finally {
      setAccepting(false);
    }
  };

  return (
    <Show
      when={preview()}
      fallback={
        <FilePick
          picked={picked()}
          onPick={setPicked}
          passphrase={passphrase()}
          onPassphrase={setPassphrase}
          busy={loading()}
          error={error()}
          onPreview={load}
        />
      }
    >
      {(current) => (
        <>
          <Show
            when={saved()}
            fallback={
              <Card aria-label={t.crumb}>
                <div class="flex flex-col gap-3 px-4 py-4 sm:flex-row sm:items-start">
                  <div class="min-w-0 flex-1">
                    <p class="font-medium text-fg">{t.into(current().base)}</p>
                    <p class="text-fg-muted text-sm">
                      {t.from(
                        current().file.revision,
                        current().file.uniwrt,
                        formatDateTime(current().file.exported_at),
                      )}
                    </p>
                    <div class="mt-2 flex flex-wrap gap-1.5" role="status">
                      <Badge tone={changes() > 0 ? "info" : "neutral"}>
                        {t.count.changes(changes())}
                      </Badge>
                      <Show when={current().nodes.length > 0}>
                        <Badge tone={current().nodes.every((n) => n.node !== null) ? "ok" : "warn"}>
                          {t.count.matched(
                            current().nodes.filter((n) => n.node !== null).length,
                            current().nodes.length,
                          )}
                        </Badge>
                      </Show>
                      <Show when={missing().size - filled().length}>
                        {(n) => <Badge tone="warn">{t.count.missing(n())}</Badge>}
                      </Show>
                    </div>
                    <p class="mt-2.5 text-fg-muted text-sm">
                      {hard().length > 0
                        ? t.blocked
                        : sorted().blocking.length > 0
                          ? t.needsSecrets
                          : changes() > 0 || filled().length > 0
                            ? t.ready(current().base + 1)
                            : t.nothing}
                    </p>
                  </div>
                  <div class="flex shrink-0 flex-wrap gap-2">
                    <Button variant="ghost" size="sm" disabled={loading()} onClick={back}>
                      {t.another}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      icon={RotateCw}
                      disabled={loading()}
                      onClick={load}
                    >
                      {loading() ? t.previewing : t.again}
                    </Button>
                  </div>
                </div>
                <Show when={current().file.secrets && !current().file.opened}>
                  <div class="flex flex-col gap-2 border-warn/40 border-t bg-warn/10 px-4 py-2.5 text-sm sm:flex-row sm:items-center">
                    <p role="status" class="min-w-0 flex-1 text-fg">
                      {t.notOpened}
                    </p>
                    <Button size="sm" icon={KeyRound} onClick={back}>
                      {t.enterPassphrase}
                    </Button>
                  </div>
                </Show>
                <Show when={stale() && live.config.head}>
                  {(head) => (
                    <p
                      role="alert"
                      class="border-warn/40 border-t bg-warn/10 px-4 py-2.5 text-fg text-sm"
                    >
                      {t.stale(head().id)}
                    </p>
                  )}
                </Show>
                <Show when={error()}>
                  <div class="px-4 pb-4">
                    <RequestError text={`${t.failed}: ${error()}`} />
                  </div>
                </Show>
                <Show when={hard().length > 0}>
                  <Issues title={t.errors} tone="crit" issues={hard()} />
                </Show>
                <Show when={sorted().warnings.length > 0}>
                  <Issues title={t.warnings} tone="warn" issues={sorted().warnings} limit={5} />
                </Show>
                <Show when={hard().length === 0 && (changes() > 0 || missing().size > 0)}>
                  <form
                    class="flex flex-col gap-3 border-edge/60 border-t px-4 py-4 sm:flex-row sm:items-end"
                    onSubmit={(event) => {
                      event.preventDefault();
                      if (canAccept()) accept();
                    }}
                  >
                    <div class="min-w-0 flex-1">
                      <TextField
                        label={t.message}
                        value={message()}
                        onInput={setMessage}
                        hint={t.messageHint}
                        disabled={accepting()}
                      />
                    </div>
                    <Button type="submit" variant="primary" icon={Download} disabled={!canAccept()}>
                      {accepting() ? t.accepting : t.accept(current().base + 1)}
                    </Button>
                  </form>
                </Show>
              </Card>
            }
          >
            {(done) => (
              <ImportDone
                id={done().id}
                base={done().base}
                label={t.crumb}
                body={strings.settings.import.done.body}
              />
            )}
          </Show>
          <Show when={saved() === undefined}>
            <FileSecrets
              secrets={current().secrets}
              values={values()}
              onValue={(path, value) => setValues((all) => ({ ...all, [path]: value }))}
              errors={sorted().bySecret}
              nodeName={nodeName}
              disabled={accepting()}
            />
          </Show>
          <Card aria-label={t.changesTitle}>
            <CardHeader
              title={t.changesTitle}
              description={t.changesHint}
              actions={<span class="tabular-nums">{t.count.changes(changes())}</span>}
            />
            <div class="px-4 pb-4">
              <Show
                when={changes() > 0}
                fallback={<p class="text-fg-muted text-sm">{t.noChanges}</p>}
              >
                <ChangeList changes={current().changes} nodeName={nodeName} />
              </Show>
            </div>
          </Card>
          <FileNodes nodes={current().nodes} kept={current().kept} nodeName={nodeName} />
        </>
      )}
    </Show>
  );
}
