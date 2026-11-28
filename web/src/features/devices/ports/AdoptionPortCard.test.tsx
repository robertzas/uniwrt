import { render, screen, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { SiteSave } from "../../../api/gen/SiteSave";
import { detail, facts } from "../../../test/fixtures";
import { siteConfig, siteView } from "../../../test/site";
import { AdoptionPortCard } from "./AdoptionPortCard";
import { addressError, adoptionPorts } from "./adoption";

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

const gateway = { ...detail(facts(1)).node, role: "gateway" as const };

describe("adoption port", () => {
  it("offers the LAN's own ports, never the WAN or a switch's", () => {
    const f = facts(1);
    expect(adoptionPorts(f, null)).toEqual(["lan1", "lan2"]);
    expect(adoptionPorts(f, "lan4")).toEqual(["lan1", "lan2", "lan4"]);
    const sw: NodeFacts = {
      ...f,
      board: {
        ...f.board,
        layout: { ...f.board.layout, switch: { name: "switch0", lan_ports: [1, 2] } },
      } as unknown as NodeFacts["board"],
    };
    expect(adoptionPorts(sw, null)).toEqual([]);
    expect(adoptionPorts(null, null)).toEqual([]);
  });

  it("checks the gateway's address on the segment", () => {
    expect(addressError("192.168.1.250")).toBeNull();
    expect(addressError(" 192.168.1.2 ")).toBeNull();
    expect(addressError("192.168.1.1")).toBe("factory");
    for (const bad of ["192.168.1.0", "192.168.1.255", "192.168.2.5", "192.168.1.07", "x"]) {
      expect(addressError(bad)).toBe("format");
    }
  });

  it("sets a port aside, refusing the factory address, then removes it", async () => {
    const user = userEvent.setup();
    const { unmount } = render(() => (
      <AdoptionPortCard node={gateway} facts={facts(1)} view={siteView(2, siteConfig())} admin />
    ));
    const card = screen.getByRole("region", { name: "Adoption port" });
    expect(within(card).getByText(/No port is set aside/)).toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Edit" }));
    await user.selectOptions(within(card).getByLabelText("Port"), "lan2");
    expect(within(card).getByText(/lan2 stops carrying your networks/)).toBeInTheDocument();
    const address = within(card).getByLabelText("The gateway's address there");
    expect(address).toHaveValue("192.168.1.250");
    await user.clear(address);
    await user.type(address, "192.168.1.1");
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(within(card).getByText(/the factory-default router's own address/)).toBeInTheDocument();
    expect(saves).toEqual([]);
    await user.clear(address);
    await user.type(address, "192.168.1.240");
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText(/Saved as revision 3/)).toBeInTheDocument();
    expect(saves[0]?.message).toBe("Adoption port lan2");
    expect(saves[0]?.config.adoption_port).toEqual({ port: "lan2", address: "192.168.1.240" });
    unmount();

    const config = siteConfig();
    config.adoption_port = { port: "lan2", address: "192.168.1.240" };
    render(() => (
      <AdoptionPortCard node={gateway} facts={facts(1)} view={siteView(3, config)} admin />
    ));
    const again = screen.getByRole("region", { name: "Adoption port" });
    expect(within(again).getByText("lan2, the gateway at 192.168.1.240")).toBeInTheDocument();
    await user.click(within(again).getByRole("button", { name: "Edit" }));
    await user.selectOptions(within(again).getByLabelText("Port"), "");
    await user.click(within(again).getByRole("button", { name: "Save" }));
    expect(await within(again).findByText(/Saved as revision 3/)).toBeInTheDocument();
    expect(saves[1]?.message).toBe("Adoption port removed");
    expect(saves[1]?.config.adoption_port).toBeNull();
  });

  it("is read-only to viewers", () => {
    render(() => (
      <AdoptionPortCard
        node={gateway}
        facts={facts(1)}
        view={siteView(2, siteConfig())}
        admin={false}
      />
    ));
    expect(screen.queryByRole("button", { name: "Edit" })).toBeNull();
  });
});
