import { Stethoscope } from "lucide";
import { Show } from "solid-js";
import { strings } from "../../../lib/strings";
import { Badge } from "../../../ui/Badge";
import { chipText, severityTones } from "./describe";
import { createDiagnosis } from "./query";

/** The Overview's chip when the doctor finds an issue in the last day: the worst, and a way in. */
export function DoctorChip(props: { mac: string; onOpen: () => void }) {
  const diagnosis = createDiagnosis(
    () => props.mac,
    () => 24,
  );
  const issues = () => (diagnosis.isError ? [] : (diagnosis.data?.issues ?? []));
  return (
    <Show when={issues()[0]}>
      {(worst) => (
        <button
          type="button"
          title={strings.client.doctor.chipHint}
          onClick={() => props.onOpen()}
          class="inline-flex w-fit items-center rounded-full border border-edge bg-surface px-3 py-1 hover:bg-raised focus-visible:outline-2 focus-visible:outline-accent"
        >
          <Badge tone={severityTones[worst().severity]} icon={Stethoscope}>
            {chipText(issues())}
          </Badge>
        </button>
      )}
    </Show>
  );
}
