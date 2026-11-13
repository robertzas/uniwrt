import type { Band } from "../../api/gen/Band";
import type { MeshConfig } from "../../api/gen/MeshConfig";
import type { MeshRouting } from "../../api/gen/MeshRouting";
import type { Network } from "../../api/gen/Network";
import { strings } from "../../lib/strings";
import { bytes, passphraseOk } from "./draft";
import { wiredVlanError } from "./summary";

const t = strings.wifi.mesh;

/** The bands the mesh can take, `auto` for none chosen (features/mesh.md). */
export const MESH_BANDS = ["auto", "2g", "5g", "6g"] as const;
export type MeshBand = (typeof MESH_BANDS)[number];
export const ROUTINGS: readonly MeshRouting[] = ["batman_v", "batman_iv"];

/** The mesh card's edit; `key` is a new key, empty to keep the current one. */
export interface MeshDraft {
  enabled: boolean;
  meshId: string;
  key: string;
  band: MeshBand;
  routing: MeshRouting;
  wired: boolean;
  vlan: string;
}

export interface MeshErrors {
  meshId?: string;
  key?: string;
  vlan?: string;
}

export function meshDraftOf(mesh: MeshConfig): MeshDraft {
  return {
    enabled: mesh.enabled,
    meshId: mesh.mesh_id,
    key: "",
    band: mesh.band === null || mesh.band === "60g" ? "auto" : mesh.band,
    routing: mesh.routing,
    wired: mesh.wired,
    vlan: String(mesh.wired_vlan),
  };
}

const bandOf = (band: MeshBand): Band | null => (band === "auto" ? null : band);

/** What the draft changes that the mesh's links depend on, in the revision message's words. */
function moved(draft: MeshDraft, mesh: MeshConfig): string[] {
  const parts: string[] = [];
  if (draft.meshId !== mesh.mesh_id) parts.push(t.parts.meshId);
  if (draft.key !== "") parts.push(t.parts.key);
  if (bandOf(draft.band) !== mesh.band) parts.push(t.parts.band);
  if (draft.routing !== mesh.routing) parts.push(t.parts.routing);
  return parts;
}

export function meshChanged(draft: MeshDraft, mesh: MeshConfig): boolean {
  return (
    draft.enabled !== mesh.enabled ||
    moved(draft, mesh).length > 0 ||
    draft.wired !== mesh.wired ||
    (draft.wired && draft.vlan.trim() !== String(mesh.wired_vlan))
  );
}

/**
 * What saving the draft does to the links over the mesh's Wi-Fi: `off` cuts them for good,
 * `moves` until every device has the change (the review names the devices this cuts off).
 */
export function meshCut(draft: MeshDraft, mesh: MeshConfig): "off" | "moves" | undefined {
  if (!mesh.enabled) return undefined;
  if (!draft.enabled) return "off";
  return moved(draft, mesh).length > 0 ? "moves" : undefined;
}

export function meshErrors(draft: MeshDraft, networks: readonly Network[]): MeshErrors {
  const errors: MeshErrors = {};
  const size = bytes(draft.meshId);
  if (size < 1 || size > 32) errors.meshId = t.meshIdError;
  if (draft.key !== "" && !passphraseOk(draft.key)) errors.key = t.keyError;
  const vlan = draft.wired ? wiredVlanError(draft.vlan, networks) : undefined;
  if (vlan) errors.vlan = vlan;
  return errors;
}

/** A key of 32 letters and digits (about 190 bits), from the browser's CSPRNG. */
export function randomMeshKey(): string {
  const alphabet = "abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
  // 256 % 56 = 32: skip the bytes that would favour the first letters.
  const out: string[] = [];
  while (out.length < 32) {
    for (const byte of crypto.getRandomValues(new Uint8Array(48))) {
      if (byte < 224 && out.length < 32) out.push(alphabet[byte % alphabet.length] ?? "");
    }
  }
  return out.join("");
}

/**
 * The mesh a valid draft saves, the secrets with it and the revision's message. Turning the mesh
 * on without a key generates one; settings edited while it is off wait for it, and wired links
 * turned off keep their VLAN.
 */
export function meshSave(
  draft: MeshDraft,
  mesh: MeshConfig,
): { mesh: MeshConfig; secrets?: Record<string, string>; message: string } {
  const key =
    draft.key !== "" ? draft.key : draft.enabled && mesh.key === null ? randomMeshKey() : "";
  const wiredVlan = draft.wired ? Number(draft.vlan.trim()) : mesh.wired_vlan;
  const next: MeshConfig = {
    ...mesh,
    enabled: draft.enabled,
    mesh_id: draft.meshId,
    band: bandOf(draft.band),
    routing: draft.routing,
    wired: draft.wired,
    wired_vlan: wiredVlan,
  };
  const words: string[] = [];
  if (draft.enabled !== mesh.enabled) words.push(draft.enabled ? t.turnedOn : t.turnedOff);
  const parts = moved(draft, mesh);
  if (parts.length > 0) words.push(t.settings(parts));
  if (draft.wired !== mesh.wired || wiredVlan !== mesh.wired_vlan) {
    words.push(t.message(draft.wired, wiredVlan));
  }
  return {
    mesh: next,
    ...(key !== "" && { secrets: { "mesh/key": key } }),
    message: words.join("; "),
  };
}
