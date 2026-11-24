import { beforeEach, describe, expect, it } from "vitest";
import { stubTerminalSocket, TerminalSocket } from "../../../test/terminalSocket";
import { describeEnd, PART, TerminalConnection, type TerminalEnd, terminalUrl } from "./connection";

let opened = 0;
let output: string[] = [];
let ends: TerminalEnd[] = [];

function connect(size?: { cols: number; rows: number }) {
  const connection = new TerminalConnection(
    "ws://test/api/v1/nodes/1/terminal?cols=80&rows=24",
    {
      opened: () => opened++,
      output: (bytes) => output.push(new TextDecoder().decode(bytes)),
      ended: (end) => ends.push(end),
    },
    size,
  );
  const ws = TerminalSocket.last;
  ws.open();
  return { connection, ws };
}

beforeEach(() => {
  stubTerminalSocket();
  opened = 0;
  output = [];
  ends = [];
});

describe("terminalUrl", () => {
  it("follows the page's scheme and host", () => {
    expect(terminalUrl(3, 120, 40, { protocol: "https:", host: "uniwrt.lan" } as Location)).toBe(
      "wss://uniwrt.lan/api/v1/nodes/3/terminal?cols=120&rows=40",
    );
    expect(terminalUrl(3, 80, 24, { protocol: "http:", host: "10.0.0.2:8080" } as Location)).toBe(
      "ws://10.0.0.2:8080/api/v1/nodes/3/terminal?cols=80&rows=24",
    );
  });
});

describe("TerminalConnection", () => {
  it("reads output as bytes and opens on the controller's word", () => {
    const { ws } = connect();
    expect(ws.binaryType).toBe("arraybuffer");
    ws.text({ type: "opened" });
    ws.output("root@gw:~# ");
    expect(opened).toBe(1);
    expect(output).toEqual(["root@gw:~# "]);
  });

  it("sends a long paste in parts the controller takes", () => {
    const { connection, ws } = connect();
    connection.send(new Uint8Array(PART * 2 + 10).fill(0x61));
    expect(ws.bytes.map((part) => part.length)).toEqual([PART, PART, 10]);
  });

  it("sends a resize only when the size changes", () => {
    const { connection, ws } = connect({ cols: 80, rows: 24 });
    connection.resize(80, 24);
    connection.resize(100, 30);
    connection.resize(100, 30);
    expect(ws.texts).toEqual([{ type: "resize", cols: 100, rows: 30 }]);
  });

  it("sends nothing before the socket opens or after it ends", () => {
    new TerminalConnection("ws://test/x", { opened() {}, output() {}, ended() {} });
    const ws = TerminalSocket.last;
    const connection = new TerminalConnection("ws://test/x", {
      opened() {},
      output() {},
      ended() {},
    });
    connection.send(new Uint8Array([1]));
    expect(TerminalSocket.last.bytes).toEqual([]);
    expect(ws.bytes).toEqual([]);
  });

  it("ends with the controller's reason and exit code", () => {
    const { ws } = connect();
    ws.text({ type: "ended", exit_code: 1, reason: "The shell ended" });
    ws.drop(1000);
    expect(ends).toEqual([{ kind: "ended", exitCode: 1, reason: "The shell ended" }]);
    expect(describeEnd(ends[0] as TerminalEnd)).toBe("The shell ended (exit code 1)");
  });

  it("ends as the session's end on 4401, and as lost on a silent close", () => {
    connect().ws.drop(4401);
    connect().ws.drop(1006);
    expect(ends).toEqual([{ kind: "session" }, { kind: "lost" }]);
    expect(ends.map(describeEnd)).toEqual([
      "Your session ended: sign in again",
      "The connection to the controller was lost",
    ]);
  });

  it("ends once when the admin disconnects", () => {
    const { connection, ws } = connect();
    connection.close();
    connection.send(new Uint8Array([1]));
    expect(ws.closedWith).toBe(1000);
    expect(ends).toEqual([{ kind: "closed" }]);
  });

  it("words an end without a reason or with a clean exit plainly", () => {
    expect(describeEnd({ kind: "ended", exitCode: 0, reason: "The shell ended" })).toBe(
      "The shell ended",
    );
    expect(describeEnd({ kind: "ended", exitCode: null, reason: null })).toBe("The shell ended");
    expect(describeEnd({ kind: "closed" })).toBe("Disconnected");
  });
});
