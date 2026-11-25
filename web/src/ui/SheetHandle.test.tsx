import { fireEvent, render } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { DISMISS_PX, SheetHandle } from "./SheetHandle";

function setup() {
  const sheet = document.createElement("section");
  const onDismiss = vi.fn();
  const { container } = render(() => <SheetHandle sheet={() => sheet} onDismiss={onDismiss} />);
  const grip = container.firstElementChild as HTMLElement;
  const at = (y: number) => ({ pointerId: 1, button: 0, clientY: y });
  return { sheet, onDismiss, grip, at };
}

describe("SheetHandle", () => {
  it("moves the sheet down with a drag, and dismisses it past the threshold", () => {
    const { sheet, onDismiss, grip, at } = setup();
    expect(grip.getAttribute("aria-hidden")).toBe("true");
    expect(grip.className).toContain("md:hidden");
    fireEvent.pointerDown(grip, at(100));
    fireEvent.pointerMove(grip, at(140));
    expect(sheet.style.transform).toBe("translateY(40px)");
    fireEvent.pointerMove(grip, at(60));
    expect(sheet.style.transform).toBe("");
    fireEvent.pointerMove(grip, at(100 + DISMISS_PX + 5));
    fireEvent.pointerUp(grip, at(100 + DISMISS_PX + 5));
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("springs back after a short drag or a cancelled one", () => {
    const { sheet, onDismiss, grip, at } = setup();
    fireEvent.pointerDown(grip, at(100));
    fireEvent.pointerMove(grip, at(130));
    fireEvent.pointerUp(grip, at(130));
    expect(sheet.style.transform).toBe("");
    expect(sheet.style.transition).toContain("transform");
    fireEvent.pointerDown(grip, at(100));
    fireEvent.pointerMove(grip, at(100 + DISMISS_PX * 2));
    fireEvent.pointerCancel(grip, at(100 + DISMISS_PX * 2));
    expect(sheet.style.transform).toBe("");
    expect(onDismiss).not.toHaveBeenCalled();
  });
});
