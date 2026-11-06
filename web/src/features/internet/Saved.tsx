import { useNavigate } from "@solidjs/router";
import { CircleCheck } from "lucide";
import { strings } from "../../lib/strings";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";

const t = strings.internet;

/** A card's change saved as a revision, with the way to review and apply it. */
export function Saved(props: { revision: number; installs?: boolean }) {
  const navigate = useNavigate();
  return (
    <div
      class="flex flex-wrap items-center gap-3 rounded-control border border-ok/40 bg-ok/10 px-3 py-2.5 text-sm"
      role="status"
    >
      <Icon icon={CircleCheck} class="size-4 shrink-0 text-ok" />
      <div class="min-w-0 flex-1">
        <p>{t.saved(props.revision)}</p>
        {props.installs && <p class="mt-0.5 text-fg-muted text-xs">{t.installs}</p>}
      </div>
      <Button size="sm" onClick={() => navigate("/changes")}>
        {t.review}
      </Button>
    </div>
  );
}
