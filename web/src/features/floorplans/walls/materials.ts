/** How each wall material is drawn on the plan (M6.2c): lightest first, as the picker lists them. */
import type { Material } from "../../../api/gen/Material";

export interface Look {
  material: Material;
  /** Plans are drawn on white, so these are fixed colors, not theme tokens. */
  color: string;
  /** Stroke width in screen pixels: heavier walls draw thicker. */
  width: number;
}

export const MATERIALS: readonly Look[] = [
  { material: "glass", color: "#38bdf8", width: 2.5 },
  { material: "drywall", color: "#94a3b8", width: 3 },
  { material: "wood", color: "#a16207", width: 3.5 },
  { material: "brick", color: "#c2410c", width: 4 },
  { material: "concrete", color: "#57534e", width: 5 },
  { material: "metal", color: "#0f172a", width: 4.5 },
];

export const look = (material: Material): Look =>
  MATERIALS.find((m) => m.material === material) ?? (MATERIALS[1] as Look);
