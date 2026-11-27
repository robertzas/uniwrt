import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createSlugger, renderChapter, renderGuide } from "./render";

describe("the guide's renderer", () => {
  it("slugs headings as GitHub does, repeats numbered", () => {
    const slug = createSlugger();
    expect(slug("Dashboard, Topology")).toBe("dashboard-topology");
    expect(slug("Wi-Fi daemon, Firmware and Installed packages")).toBe(
      "wi-fi-daemon-firmware-and-installed-packages",
    );
    expect(slug("Review & overwrite")).toBe("review--overwrite");
    expect(slug("Schedules")).toBe("schedules");
    expect(slug("Schedules")).toBe("schedules-1");
    expect(slug("Schedules")).toBe("schedules-2");
  });

  it("anchors headings, lists the sections and rewrites links", () => {
    const c = renderChapter(
      "devices",
      [
        "# Devices and *adoption*",
        "## The `Config` tab",
        "See [Wi-Fi](wifi.md#roaming), [below](#the-config-tab), [the index](README.md) and [releases](https://example.com/r).",
      ].join("\n\n"),
    );
    expect(c.title).toBe("Devices and adoption");
    expect(c.sections).toEqual([{ anchor: "the-config-tab", text: "The Config tab", depth: 2 }]);
    expect(c.html).toContain('<h2 data-anchor="the-config-tab">The <code>Config</code> tab</h2>');
    expect(c.html).toContain('<a href="/help/wifi#roaming">Wi-Fi</a>');
    expect(c.html).toContain('<a href="/help/devices#the-config-tab">below</a>');
    expect(c.html).toContain('<a href="/help">the index</a>');
    expect(c.html).toContain(
      '<a href="https://example.com/r" target="_blank" rel="noopener noreferrer">releases</a>',
    );
  });

  it("escapes raw HTML and turns images into links to the online copy", () => {
    const c = renderChapter("x", '<b style="color:red">hi</b>\n\n![The map](img/map.png)');
    expect(c.html).not.toContain("<b");
    expect(c.html).toContain("&#60;b");
    expect(c.html).toContain(
      '<a href="https://robertzas.github.io/uniwrt/guide/img/map.png" target="_blank" rel="noopener noreferrer">The map</a>',
    );
  });

  it("refuses links it can't place, and broken ones", () => {
    expect(() => renderChapter("x", "[a](../features/ui.md)")).toThrow(/neither a chapter/);
    expect(() =>
      renderGuide({ "README.md": "# Guide\n\n[A](a.md#nope)", "a.md": "# A\n\n## Yes" }),
    ).toThrow("README.md → a#nope");
    expect(() => renderGuide({ "README.md": "[B](b.md)" })).toThrow("README.md → b");
  });

  it("orders chapters as the index links them", () => {
    const chapters = renderGuide({
      "z.md": "# Z",
      "b.md": "# B",
      "a.md": "# A",
      "README.md": "# Guide\n\n[B](b.md) [A](a.md) [B again](b.md)",
    });
    expect(chapters.map((c) => c.slug)).toEqual(["index", "b", "a", "z"]);
  });

  it("renders the real guide, every link resolved", () => {
    const dir = join(import.meta.dirname, "../../docs/guide");
    const files = Object.fromEntries(
      readdirSync(dir).map((name) => [name, readFileSync(join(dir, name), "utf8")]),
    );
    const chapters = renderGuide(files);
    expect(chapters[0]?.slug).toBe("index");
    expect(chapters[1]?.slug).toBe("install");
    expect(chapters).toHaveLength(Object.keys(files).filter((n) => n.endsWith(".md")).length);
    for (const c of chapters) expect(c.title, c.slug).not.toBe(c.slug);
  });
});
