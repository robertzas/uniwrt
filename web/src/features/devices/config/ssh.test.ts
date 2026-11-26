import { describe, expect, it } from "vitest";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import type { SshServerFacts } from "../../../api/gen/SshServerFacts";
import { siteView } from "../../../test/site";
import { managementNetwork, sshNow, sshOf, withSsh } from "./ssh";

const OFF = { key_only: false, management_only: false };

function site(): SiteConfig {
  return siteView().config;
}

function server(password: boolean, device: string | null, running = true): SshServerFacts {
  return { running, password, root_password: password, device, listen: ["22"] };
}

describe("SSH hardening", () => {
  it("manages dropbear when any of it goes on, and keeps it to undo it", () => {
    const on = withSsh(site(), 7, { key_only: true, management_only: false });
    const entry = on.nodes.find((n) => n.node === 7);
    expect(entry?.managed).toEqual(["dropbear"]);
    expect(sshOf(on, 7)).toEqual({ key_only: true, management_only: false });
    const off = withSsh(on, 7, OFF);
    expect(off.nodes.find((n) => n.node === 7)?.managed).toEqual(["dropbear"]);
    expect(sshOf(off, 7)).toEqual(OFF);
    expect(sshOf(site(), 99)).toEqual(OFF);
  });

  it("binds to the management network, else the default one", () => {
    const config = site();
    expect(managementNetwork(config)?.purpose).toBe("default");
    const lan = config.networks[0];
    if (!lan) throw new Error("the site has a network");
    const mgmt = {
      ...lan,
      id: "mgmt",
      name: "Management",
      purpose: "mgmt" as const,
    };
    expect(managementNetwork({ ...config, networks: [...config.networks, mgmt] })?.name).toBe(
      "Management",
    );
  });

  it("reads how the running servers let people in", () => {
    expect(sshNow(null)).toBeNull();
    expect(sshNow([server(true, null, false)])).toBeNull();
    expect(sshNow([server(true, null)])).toEqual({ passwords: true, devices: [] });
    expect(sshNow([server(false, "br-lan.99"), server(false, "br-lan.99")])).toEqual({
      passwords: false,
      devices: ["br-lan.99"],
    });
    expect(sshNow([server(false, "br-lan"), server(false, null)])).toEqual({
      passwords: false,
      devices: [],
    });
  });
});
