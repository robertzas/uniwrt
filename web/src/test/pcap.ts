/** A `.pcap`: the file header, then a record per length, in the given byte order. */
export function pcap(lengths: readonly number[], little = true): Uint8Array {
  const size = 24 + lengths.reduce((sum, length) => sum + 16 + length, 0);
  const bytes = new Uint8Array(size);
  const view = new DataView(bytes.buffer);
  view.setUint32(0, 0xa1b2c3d4, little);
  let offset = 24;
  for (const length of lengths) {
    view.setUint32(offset + 8, length, little);
    view.setUint32(offset + 12, length, little);
    offset += 16 + length;
  }
  return bytes;
}
