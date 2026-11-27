import { Pencil, Plus, RotateCw, Trash2 } from "lucide";
import { createMemo, createSignal, onSettled, Show, useContext } from "solid-js";
import type { Account } from "../../../api/gen/Account";
import { api, errorText } from "../../../lib/api";
import { formatDateTime, formatDuration } from "../../../lib/format";
import { SessionContext } from "../../../lib/session";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { Button } from "../../../ui/Button";
import { Card, CardHeader } from "../../../ui/Card";
import { Dialog, DialogFooter } from "../../../ui/Dialog";
import { type Column, Table } from "../../../ui/Table";
import { showToast } from "../../../ui/Toast";
import { UserDialog } from "./UserDialog";

const t = strings.settings.users;
/** Sessions record their use at most this often (security.md), so "now" spans it. */
const ACTIVE_MS = 5 * 60_000;

/**
 * Settings → Users (security.md, Users; D-153), for admins: who can sign in, with their role and
 * last activity; admins add users, change a role or set a password (ending that user's sessions)
 * and delete them. The only admin stays one, and nobody deletes themselves, as the API enforces.
 */
export function UsersTab(props: { admin: boolean }) {
  return (
    <Show
      when={props.admin}
      fallback={
        <Card aria-label={t.title}>
          <CardHeader title={t.title} description={t.description} />
          <p class="px-4 pb-4 text-fg-subtle text-sm">{t.adminOnly}</p>
        </Card>
      }
    >
      <Users />
    </Show>
  );
}

function Users() {
  const session = useContext(SessionContext);
  const [users, setUsers] = createSignal<readonly Account[] | undefined>();
  const [failed, setFailed] = createSignal(false);
  const [now, setNow] = createSignal(Date.now());
  const [editing, setEditing] = createSignal<Account | "new" | undefined>();
  const [removing, setRemoving] = createSignal<Account | undefined>();
  const [busy, setBusy] = createSignal(false);

  const load = (signal?: AbortSignal) =>
    api.users
      .list(signal)
      .then((answer) => {
        if (!Array.isArray(answer)) throw new Error("not a user list");
        setUsers(answer);
        setFailed(false);
        setNow(Date.now());
      })
      .catch(() => {
        if (!signal?.aborted && users() === undefined) setFailed(true);
      });
  onSettled(() => {
    const controller = new AbortController();
    load(controller.signal);
    return () => controller.abort();
  });

  const admins = createMemo(() => (users() ?? []).filter((u) => u.role === "admin").length);
  const self = (user: Account) => user.id === session.user.id;
  const lastActive = (user: Account) => {
    if (user.last_seen_at === null) return t.notSignedIn;
    const since = now() - user.last_seen_at;
    return since < ACTIVE_MS ? t.activeNow : t.ago(formatDuration(since / 1000));
  };

  const columns: Column<Account>[] = [
    {
      id: "username",
      header: t.columns.username,
      sort: (a, b) => a.username.localeCompare(b.username),
      cell: (user) => (
        <span class="flex items-center gap-2">
          <span class="truncate font-medium">{user.username}</span>
          <Show when={self(user)}>
            <Badge tone="info">{t.you}</Badge>
          </Show>
        </span>
      ),
    },
    {
      id: "role",
      header: t.columns.role,
      sort: (a, b) => a.role.localeCompare(b.role),
      cell: (user) => strings.account.role[user.role],
    },
    {
      id: "lastActive",
      header: t.columns.lastActive,
      sort: (a, b) => (b.last_seen_at ?? 0) - (a.last_seen_at ?? 0),
      cell: (user) => (
        <span
          class="text-fg-muted"
          title={user.last_seen_at === null ? undefined : formatDateTime(user.last_seen_at)}
        >
          {lastActive(user)}
        </span>
      ),
    },
    {
      id: "added",
      header: t.columns.added,
      sort: (a, b) => a.created_at - b.created_at,
      cell: (user) => <span class="text-fg-muted">{formatDateTime(user.created_at)}</span>,
    },
    {
      id: "actions",
      header: t.columns.actions,
      align: "end",
      card: "aside",
      cell: (user) => (
        <span class="inline-flex gap-1">
          <Button
            size="sm"
            variant="ghost"
            icon={Pencil}
            aria-label={t.edit(user.username)}
            title={t.edit(user.username)}
            onClick={() => setEditing(user)}
          />
          <Button
            size="sm"
            variant="ghost"
            icon={Trash2}
            aria-label={t.remove(user.username)}
            title={self(user) ? t.cantRemoveSelf : t.remove(user.username)}
            disabled={self(user) || busy()}
            onClick={() => setRemoving(user)}
          />
        </span>
      ),
    },
  ];

  const remove = async (user: Account) => {
    setRemoving(undefined);
    setBusy(true);
    try {
      await api.users.remove(user.id);
      showToast({ title: t.removed(user.username), tone: "info" });
      await load();
    } catch (err) {
      showToast({ title: t.changeFailed, description: errorText(err), tone: "crit" });
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Card class="overflow-hidden" aria-label={t.title}>
        <CardHeader
          title={t.title}
          description={
            users() === undefined ? t.description : t.count(users()?.length ?? 0, admins())
          }
          actions={
            <Button variant="primary" icon={Plus} onClick={() => setEditing("new")}>
              {t.add}
            </Button>
          }
        />
        <Show
          when={users()}
          fallback={
            <Show
              when={failed()}
              fallback={
                <p role="status" class="py-16 text-center text-fg-subtle text-sm">
                  {t.loading}…
                </p>
              }
            >
              <div role="alert" class="flex flex-col items-center gap-3 py-12 text-sm">
                <p class="text-fg-muted">{t.failed}</p>
                <Button size="sm" icon={RotateCw} onClick={() => load()}>
                  {t.retry}
                </Button>
              </div>
            </Show>
          }
        >
          {(list) => (
            <Table
              label={t.title}
              columns={columns}
              rows={list()}
              rowKey={(user) => String(user.id)}
              sortBy={{ column: "username" }}
            />
          )}
        </Show>
      </Card>
      <Show when={editing()}>
        {(target) => {
          const user = () => (target() === "new" ? undefined : (target() as Account));
          return (
            <UserDialog
              user={user()}
              self={user() !== undefined && self(user() as Account)}
              onlyAdmin={user()?.role === "admin" && admins() === 1}
              onClose={() => setEditing(undefined)}
              onSaved={(saved, changed) => {
                setEditing(undefined);
                showToast({
                  title: user() ? t.saved(saved.username) : t.added(saved.username),
                  description:
                    changed && user() && !self(user() as Account) ? t.savedHint : undefined,
                  tone: "ok",
                });
                void load();
              }}
            />
          );
        }}
      </Show>
      <Show when={removing()}>
        {(user) => (
          <Dialog
            open
            onOpenChange={(open) => {
              if (!open) setRemoving(undefined);
            }}
            title={t.removeTitle(user().username)}
            description={t.removeDetail}
          >
            <DialogFooter class="px-5 pt-2 pb-4">
              <Button variant="ghost" onClick={() => setRemoving(undefined)}>
                {t.dialog.cancel}
              </Button>
              <Button variant="primary" icon={Trash2} onClick={() => void remove(user())}>
                {t.removeConfirm}
              </Button>
            </DialogFooter>
          </Dialog>
        )}
      </Show>
    </>
  );
}
