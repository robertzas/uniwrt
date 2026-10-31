import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { Plus } from "lucide";
import { createSignal } from "solid-js";
import { describe, expect, it } from "vitest";
import { Badge } from "./Badge";
import { Button } from "./Button";
import { Dialog } from "./Dialog";
import { Sparkline } from "./Sparkline";
import { Stat } from "./Stat";
import { Tab, TabList, TabPanel, Tabs } from "./Tabs";
import { showToast, Toaster } from "./Toast";

describe("Button", () => {
  it("never submits by accident, and is square without a label", () => {
    render(() => (
      <>
        <Button icon={Plus}>Adopt</Button>
        <Button icon={Plus} aria-label="Add" variant="ghost" size="sm" />
      </>
    ));
    const adopt = screen.getByRole("button", { name: "Adopt" });
    expect(adopt).toHaveAttribute("type", "button");
    expect(adopt).toHaveClass("px-3");
    expect(screen.getByRole("button", { name: "Add" })).toHaveClass("size-7");
  });
});

describe("Badge", () => {
  it("colors the dot and keeps the label readable", () => {
    const { container } = render(() => <Badge tone="ok">Online</Badge>);
    expect(screen.getByText("Online")).toHaveClass("text-fg");
    expect(container.querySelector(".bg-ok")).toHaveAttribute("aria-hidden", "true");
  });
});

describe("Stat and Sparkline", () => {
  it("shows the value with its unit and a labelled trend", () => {
    render(() => (
      <Stat label="Throughput" value={78} unit="Mb/s">
        <Sparkline values={[10, 30, 20]} label="Throughput, last 30 minutes" />
      </Stat>
    ));
    expect(screen.getByText("Throughput")).toBeInTheDocument();
    expect(screen.getByText("Mb/s")).toBeInTheDocument();
    const spark = screen.getByRole("img", { name: "Throughput, last 30 minutes" });
    const [area, line] = spark.querySelectorAll("path");
    expect(line?.getAttribute("d")).toMatch(/^M0\.00 [\d.]+L50\.00 [\d.]+L100\.00 [\d.]+$/);
    expect(area?.getAttribute("d")).toMatch(/Z$/);
  });

  it("draws nothing for fewer than two values", () => {
    render(() => <Sparkline values={[5]} label="Load" />);
    expect(screen.getByRole("img", { name: "Load" }).querySelector("path")).toBeNull();
  });
});

describe("Dialog", () => {
  // Relies on the Kobalte patch (D-045).
  it("closes with Escape and returns focus to the trigger", async () => {
    const user = userEvent.setup();
    const [open, setOpen] = createSignal(false);
    render(() => (
      <>
        <Button onClick={() => setOpen(true)}>Open</Button>
        <Dialog open={open()} onOpenChange={setOpen} title="Reboot ap-upstairs?">
          <p>Clients roam to other access points.</p>
        </Dialog>
      </>
    ));
    const trigger = screen.getByRole("button", { name: "Open" });
    await user.click(trigger);
    const dialog = await screen.findByRole("dialog", { name: "Reboot ap-upstairs?" });
    // Under load, Kobalte moves focus into the dialog a tick after it appears; Escape before that
    // would leave no focus to restore.
    await waitFor(() => expect(dialog).toContainElement(document.activeElement as HTMLElement));
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    // Kobalte restores focus after the dialog unmounts, a tick later under load.
    await waitFor(() => expect(trigger).toHaveFocus());
  });
});

describe("Tabs", () => {
  it("switches panels with the arrow keys", async () => {
    const user = userEvent.setup();
    render(() => (
      <Tabs defaultValue="overview" aria-label="Device">
        <TabList>
          <Tab value="overview">Overview</Tab>
          <Tab value="radios">Radios</Tab>
        </TabList>
        <TabPanel value="overview">Overview panel</TabPanel>
        <TabPanel value="radios">Radios panel</TabPanel>
      </Tabs>
    ));
    await user.click(screen.getByRole("tab", { name: "Overview" }));
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Radios" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveTextContent("Radios panel");
  });
});

describe("Toast", () => {
  it("announces a toast in the notifications region", async () => {
    render(() => <Toaster />);
    showToast({ title: "Adopted ap-upstairs", description: "Watching it live.", tone: "ok" });
    expect(await screen.findByText("Adopted ap-upstairs")).toBeInTheDocument();
    expect(screen.getByText("Watching it live.")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: /Notifications/ })).toBeInTheDocument();
  });
});
