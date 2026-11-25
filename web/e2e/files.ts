import { deflateSync } from "node:zlib";

/*
 * Files the specs upload, made in memory: a sysupgrade image as fwtool leaves it, and a plain PNG.
 */

/** A chunk as fwtool appends it: the data, then a 16-byte trailer (`FWx0`, a CRC, the type, the size). */
function chunk(kind: number, data: Buffer): Buffer {
  const trailer = Buffer.alloc(16);
  trailer.write("FWx0", 0, "latin1");
  trailer.writeUInt32BE(0x1234_5678, 4);
  trailer[8] = kind;
  trailer.writeUInt32BE(data.length + 16, 12);
  return Buffer.concat([data, trailer]);
}

/** A sysupgrade image for `board` (`glinet,gl-mt3000`) of OpenWrt `release`: its metadata and a signature chunk. */
export function sysupgrade(board: string, release: string, target = "mediatek/filogic"): Buffer {
  const metadata = JSON.stringify({
    metadata_version: "1.1",
    compat_version: "1.0",
    supported_devices: [board],
    version: {
      release,
      date: "20261101000000",
      dist: "OpenWrt",
      version: release,
      revision: "r33400-0123456789",
      target,
      board: board.replace(",", "_"),
    },
  });
  return Buffer.concat([
    Buffer.alloc(4096, 0xff),
    chunk(1, Buffer.concat([Buffer.alloc(8), Buffer.from(metadata)])),
    chunk(0, Buffer.from("# e2e signature")),
  ]);
}

function crc32(bytes: Buffer): number {
  let crc = 0xffff_ffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) crc = crc & 1 ? (crc >>> 1) ^ 0xedb8_8320 : crc >>> 1;
  }
  return (crc ^ 0xffff_ffff) >>> 0;
}

/** A light grey RGB PNG of `width` × `height`. */
export function png(width: number, height: number): Buffer {
  const part = (type: string, data: Buffer) => {
    const length = Buffer.alloc(4);
    length.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type, "latin1"), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(body));
    return Buffer.concat([length, body, crc]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8; // bits per channel
  header[9] = 2; // RGB
  const row = width * 3 + 1;
  const pixels = Buffer.alloc(row * height, 0xee);
  for (let y = 0; y < height; y++) pixels[y * row] = 0; // no filter
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    part("IHDR", header),
    part("IDAT", deflateSync(pixels)),
    part("IEND", Buffer.alloc(0)),
  ]);
}
