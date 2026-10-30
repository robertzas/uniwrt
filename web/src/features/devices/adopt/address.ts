import type { ProbeRequest } from "../../../api/gen/ProbeRequest";

const HOST = /^[A-Za-z0-9._:%-]+$/;

/**
 * Where to reach a device, from what the user typed: a host name or an IP address, with an
 * optional port (`host:2222`, `[2001:db8::1]:2222`). Port 22 is left out. `undefined` when it is
 * not one; the server checks the same rules.
 */
export function parseAddress(input: string): ProbeRequest | undefined {
  const text = input.trim();
  let host = text;
  let port: string | undefined;
  const bracketed = /^\[([^\]]+)\](?::(\d+))?$/.exec(text);
  const withPort = /^([^:[\]]+):(\d+)$/.exec(text);
  if (bracketed?.[1]) {
    host = bracketed[1];
    port = bracketed[2];
  } else if (withPort?.[1]) {
    host = withPort[1];
    port = withPort[2];
  }
  if (!host || host.length > 253 || host.startsWith("-") || !HOST.test(host)) return undefined;
  if (port === undefined) return { host };
  const number = Number(port);
  if (!Number.isInteger(number) || number < 1 || number > 65535) return undefined;
  return number === 22 ? { host } : { host, port: number };
}

/** How the user would write the address back: `host`, `host:port` or `[v6]:port`. */
export function formatAddress(host: string, port = 22): string {
  if (port === 22) return host;
  return host.includes(":") ? `[${host}]:${port}` : `${host}:${port}`;
}
