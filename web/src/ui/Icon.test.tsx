import { render } from "@solidjs/testing-library";
import { Router } from "lucide";
import { describe, expect, it } from "vitest";
import { Icon } from "./Icon";

describe("Icon", () => {
  it("draws the icon's shapes as SVG elements", () => {
    const { container } = render(() => <Icon icon={Router} class="size-4" />);
    const svg = container.querySelector("svg");
    expect(svg).toHaveClass("size-4");
    expect(svg).toHaveAttribute("aria-hidden", "true");
    const shapes = svg?.querySelectorAll("rect, path") ?? [];
    expect(shapes).toHaveLength(Router.length);
    for (const shape of shapes) {
      expect(shape.namespaceURI).toBe("http://www.w3.org/2000/svg");
    }
    expect(svg?.querySelector("rect")).toHaveAttribute("rx", "2");
  });
});
