import { strings } from "../../lib/strings";
import { Segmented } from "../../ui/Segmented";
import { type Preset, presets } from "./ranges";

/** The preset ranges as a toggle group: `1h`, `24h`, … each named in full for screen readers. */
export function RangePicker(props: {
  value: Preset | undefined;
  onChange: (preset: Preset) => void;
  options?: readonly Preset[] | undefined;
}) {
  return (
    <Segmented
      label={strings.chart.range}
      value={props.value ?? ("" as Preset)}
      options={(props.options ?? presets).map((value) => ({
        value,
        label: value,
        description: strings.chart.rangeNames[value],
      }))}
      onChange={props.onChange}
    />
  );
}
