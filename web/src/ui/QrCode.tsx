import { createMemo } from "solid-js";
import { encode } from "uqr";

/** The modules of a QR code as one SVG path, a unit square each, inside a quiet zone. */
export function qrPath(text: string): { size: number; d: string } {
  const code = encode(text, { ecc: "M", border: 2 });
  let d = "";
  code.data.forEach((row, y) => {
    row.forEach((dark, x) => {
      if (dark) d += `M${x} ${y}h1v1h-1z`;
    });
  });
  return { size: code.size, d };
}

/** `text` as a QR code for a phone's camera: dark on white whatever the theme. */
export function QrCode(props: { text: string; label: string; class?: string }) {
  const code = createMemo(() => qrPath(props.text));
  return (
    <svg
      viewBox={`0 0 ${code().size} ${code().size}`}
      class={["rounded-control bg-white", props.class ?? "size-56"]}
      role="img"
      aria-label={props.label}
      shape-rendering="crispEdges"
    >
      <path d={code().d} fill="#000" />
    </svg>
  );
}
