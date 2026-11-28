import { render, screen, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SiteSave } from "../../api/gen/SiteSave";
import { siteConfig, siteView } from "../../test/site";
import { BandsCard } from "./BandsCard";

let saves: SiteSave[] = [];

beforeEach(() => {
  saves = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (url === "/api/v1/site" && init?.method === "PUT") {
        const body = JSON.parse(String(init.body)) as SiteSave;
        saves.push(body);
        return Response.json(siteView(3, body.config));
      }
      return Response.json(siteView(2, siteConfig()));
    }),
  );
});

describe("Wi-Fi › Radios", () => {
  it("fixes a band's channel and power, keeping the others", async () => {
    const user = userEvent.setup();
    render(() => <BandsCard view={siteView(2, siteConfig())} admin />);
    const card = screen.getByRole("region", { name: "Radios" });
    const g5 = within(card).getByRole("row", { name: "5 GHz" });
    expect(within(g5).getByText("80 MHz")).toBeInTheDocument();
    expect(within(g5).getAllByText("Auto")).toHaveLength(2);

    await user.click(within(card).getByRole("button", { name: "Edit" }));
    await user.selectOptions(within(card).getByLabelText("Channel, 5 GHz"), "36");
    expect(within(card).getByText(/A fixed channel puts every radio/)).toBeVisible();
    await user.selectOptions(within(card).getByLabelText("Width, 5 GHz"), "40");
    await user.selectOptions(within(card).getByLabelText("Transmit power, 2.4 GHz"), "14");
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText(/Saved as revision 3/)).toBeVisible();
    expect(saves[0]?.message).toBe("Radio defaults changed");
    expect(saves[0]?.config.radio.bands).toEqual([
      { band: "2g", channel: "auto", width: 20, tx_power: { dbm: 14 } },
      { band: "5g", channel: { fixed: 36 }, width: 40, tx_power: "auto" },
      { band: "6g", channel: "auto", width: 160, tx_power: "auto" },
    ]);
  });

  it("is read-only to viewers", () => {
    render(() => <BandsCard view={siteView(2, siteConfig())} admin={false} />);
    expect(screen.queryByRole("button", { name: "Edit" })).toBeNull();
  });
});
