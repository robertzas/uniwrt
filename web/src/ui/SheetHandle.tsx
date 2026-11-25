/** How far the grip is dragged down before a release dismisses the sheet, in CSS pixels. */
export const DISMISS_PX = 72;

/**
 * The grip on top of a bottom sheet, below `md` only (D-148): the sheet follows a downward drag,
 * and a release past `DISMISS_PX` dismisses it (`onDismiss`), else it springs back. A pointer
 * aid only: the sheet's close button does the same for keyboards and screen readers.
 */
export function SheetHandle(props: {
  sheet: () => HTMLElement | undefined;
  onDismiss: () => void;
}) {
  let start: { pointer: number; y: number } | undefined;
  let offset = 0;

  const place = (dy: number, settle: boolean) => {
    const sheet = props.sheet();
    if (!sheet) return;
    sheet.style.transition = settle ? "transform 160ms ease-out" : "none";
    sheet.style.transform = dy > 0 ? `translateY(${dy}px)` : "";
  };
  const end = (event: PointerEvent, dismiss: boolean) => {
    if (start?.pointer !== event.pointerId) return;
    start = undefined;
    if (dismiss && offset >= DISMISS_PX) {
      props.onDismiss();
      return;
    }
    place(0, true);
    offset = 0;
  };

  return (
    <div
      aria-hidden="true"
      class="flex h-5 shrink-0 cursor-grab touch-none items-center justify-center active:cursor-grabbing md:hidden"
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        start = { pointer: event.pointerId, y: event.clientY };
        offset = 0;
        event.currentTarget.setPointerCapture?.(event.pointerId);
      }}
      onPointerMove={(event) => {
        if (start?.pointer !== event.pointerId) return;
        offset = Math.max(0, event.clientY - start.y);
        place(offset, false);
      }}
      onPointerUp={(event) => end(event, true)}
      onPointerCancel={(event) => end(event, false)}
    >
      <span class="h-1 w-10 rounded-full bg-edge-strong" />
    </div>
  );
}
