import { render, screen, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import type { SiteSave } from "../../api/gen/SiteSave";
import { siteConfig, siteView } from "../../test/site";
import { MeshCard } from "./MeshCard";

let saves: SiteSave[] = [];

/** The test site with an IoT network on VLAN 30. */
function config(): SiteConfig {
  const base = siteConfig();
  const lan = base.networks[0];
  if (!lan) throw new Error("no LAN");
  return { ...base, networks: [...base.networks, { ...lan, id: "iot", name: "IoT", vlan: 30 }] };
}

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
      return Response.json(siteView(2, config()));
    }),
  );
});

describe("Wi-Fi › Mesh backhaul", () => {
  it("moves the wired backhaul's VLAN, refusing one a network uses", async () => {
    const user = userEvent.setup();
    render(() => <MeshCard view={siteView(2, config())} admin />);
    const card = screen.getByRole("region", { name: "Mesh backhaul" });
    expect(within(card).getByLabelText("Mesh over cables")).toBeChecked();
    const vlan = within(card).getByLabelText("Backhaul VLAN");
    expect(vlan).toHaveValue("4094");
    expect(within(card).queryByRole("button", { name: "Save" })).toBeNull();

    await user.clear(vlan);
    await user.type(vlan, "30");
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(within(card).getByText("IoT uses this VLAN: choose another")).toBeVisible();
    expect(saves).toEqual([]);

    await user.clear(vlan);
    await user.type(vlan, "4000");
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText(/Saved as revision 3/)).toBeVisible();
    expect(saves[0]?.message).toBe("Mesh over cables on VLAN 4000");
    expect(saves[0]?.config.mesh).toMatchObject({ wired: true, wired_vlan: 4000 });
  });

  it("turns wired links off, keeping the VLAN, and is read-only to viewers", async () => {
    const user = userEvent.setup();
    const view = siteView(2, config());
    view.issues = [{ path: "mesh/wired_vlan", severity: "error", message: "IoT uses VLAN 4094" }];
    const { unmount } = render(() => <MeshCard view={view} admin />);
    const card = screen.getByRole("region", { name: "Mesh backhaul" });
    expect(within(card).getByText("IoT uses VLAN 4094")).toBeVisible();
    await user.click(within(card).getByLabelText("Mesh over cables"));
    expect(within(card).queryByLabelText("Backhaul VLAN")).toBeNull();
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText(/Saved as revision 3/)).toBeVisible();
    expect(saves[0]?.message).toBe("Mesh over cables off");
    expect(saves[0]?.config.mesh).toMatchObject({ wired: false, wired_vlan: 4094 });
    unmount();

    const off = config();
    off.mesh.enabled = false;
    render(() => <MeshCard view={siteView(2, off)} admin={false} />);
    expect(screen.getByText(/The mesh is off/)).toBeVisible();
    expect(screen.getByLabelText("Mesh over cables")).toBeDisabled();
    expect(screen.getByLabelText("Backhaul VLAN")).toBeDisabled();
  });
});
