//! Vendors by MAC address: IEEE's MA-L, MA-M and MA-S registries, embedded compressed
//! (`data/oui.bin`, which `scripts/oui.py` makes; D-053) and unpacked on first use. The most
//! specific assignment wins: a 36-bit MA-S block, then a 28-bit MA-M one, then the 24-bit MA-L.

use std::sync::OnceLock;

use uniwrt_core::MacAddr;

static PACKED: &[u8] = include_bytes!("../data/oui.bin");
static TABLE: OnceLock<Option<Table>> = OnceLock::new();

const MAGIC: &[u8] = b"UOUI\x01";
/// Each entry's name index, little-endian.
const INDEX_BYTES: usize = 3;

/// The organization the address's block is assigned to, with its name normalized (`Apple`,
/// `Espressif`); `None` for randomized and multicast addresses, and for unassigned or private
/// blocks.
pub fn vendor(mac: MacAddr) -> Option<&'static str> {
    if mac.is_locally_administered() || mac.is_multicast() {
        return None;
    }
    TABLE.get_or_init(Table::unpack).as_ref()?.lookup(mac)
}

struct Table {
    bytes: Box<[u8]>,
    /// Where each name's length byte is.
    names: Box<[u32]>,
    /// MA-S, MA-M, MA-L.
    blocks: [Block; 3],
}

#[derive(Clone, Copy)]
struct Block {
    start: usize,
    len: usize,
    /// The prefix's bytes, big-endian.
    width: usize,
    /// How far an address (as a 48-bit number) shifts right to give the prefix.
    shift: u32,
}

impl Table {
    /// `None` if the embedded table is damaged, which its test rules out.
    fn unpack() -> Option<Self> {
        let bytes = miniz_oxide::inflate::decompress_to_vec_zlib(PACKED)
            .ok()?
            .into_boxed_slice();
        let mut at = MAGIC.len();
        if bytes.get(..at)? != MAGIC {
            return None;
        }
        let count = read_u32(&bytes, &mut at)?;
        let mut names = Vec::with_capacity(count);
        for _ in 0..count {
            names.push(u32::try_from(at).ok()?);
            let len = usize::from(*bytes.get(at)?);
            std::str::from_utf8(bytes.get(at + 1..at + 1 + len)?).ok()?;
            at += 1 + len;
        }
        let mut block = |width: usize, shift: u32| {
            let len = read_u32(&bytes, &mut at)?;
            let start = at;
            at = at.checked_add(len.checked_mul(width + INDEX_BYTES)?)?;
            (at <= bytes.len()).then_some(Block {
                start,
                len,
                width,
                shift,
            })
        };
        let large = block(3, 24)?;
        let medium = block(4, 20)?;
        let small = block(5, 12)?;
        Some(Self {
            bytes,
            names: names.into_boxed_slice(),
            blocks: [small, medium, large],
        })
    }

    fn lookup(&'static self, mac: MacAddr) -> Option<&'static str> {
        let address = mac
            .octets()
            .iter()
            .fold(0_u64, |acc, octet| acc << 8 | u64::from(*octet));
        self.blocks
            .iter()
            .find_map(|block| self.find(*block, address >> block.shift))
    }

    /// Binary search of one block for `prefix`.
    fn find(&'static self, block: Block, prefix: u64) -> Option<&'static str> {
        let entry = block.width + INDEX_BYTES;
        let (mut low, mut high) = (0, block.len);
        while low < high {
            let mid = low + (high - low) / 2;
            let at = block.start + mid * entry;
            let key = self.bytes[at..at + block.width]
                .iter()
                .fold(0_u64, |acc, byte| acc << 8 | u64::from(*byte));
            match key.cmp(&prefix) {
                std::cmp::Ordering::Less => low = mid + 1,
                std::cmp::Ordering::Greater => high = mid,
                std::cmp::Ordering::Equal => {
                    let index = &self.bytes[at + block.width..at + entry];
                    let index = usize::from(index[0])
                        | usize::from(index[1]) << 8
                        | usize::from(index[2]) << 16;
                    return self.name(index);
                }
            }
        }
        None
    }

    fn name(&'static self, index: usize) -> Option<&'static str> {
        let at = usize::try_from(*self.names.get(index)?).ok()?;
        let len = usize::from(self.bytes[at]);
        std::str::from_utf8(&self.bytes[at + 1..at + 1 + len]).ok()
    }
}

fn read_u32(bytes: &[u8], at: &mut usize) -> Option<usize> {
    let raw: [u8; 4] = bytes.get(*at..*at + 4)?.try_into().ok()?;
    *at += 4;
    usize::try_from(u32::from_le_bytes(raw)).ok()
}

#[cfg(test)]
mod tests {
    use uniwrt_core::MacAddr;

    use super::{TABLE, Table, vendor};

    fn of(mac: &str) -> Option<&'static str> {
        vendor(mac.parse::<MacAddr>().unwrap())
    }

    #[test]
    fn the_embedded_table_unpacks_whole() {
        let table = TABLE.get_or_init(Table::unpack).as_ref().unwrap();
        assert!(table.names.len() > 20_000, "{}", table.names.len());
        assert!(table.blocks[2].len > 30_000, "MA-L");
        assert!(table.blocks[1].len > 5_000, "MA-M");
        assert!(table.blocks[0].len > 5_000, "MA-S");
    }

    #[test]
    fn the_most_specific_assignment_names_the_vendor() {
        assert_eq!(of("b8:27:eb:12:34:56"), Some("Raspberry Pi"));
        assert_eq!(of("24:0a:c4:00:00:01"), Some("Espressif"));
        assert_eq!(of("00:00:0c:aa:bb:cc"), Some("Cisco"));
        // MA-M and MA-S blocks inside the IEEE's own MA-L assignments.
        assert_eq!(
            of("c8:5c:e2:70:00:01"),
            Some("Synergy Systems and Solutions")
        );
        assert_eq!(of("8c:1f:64:af:a0:01"), Some("Data Electronic Devices"));
        assert_eq!(of("8c:1f:64:00:20:01"), None, "an unassigned MA-S block");
    }

    #[test]
    fn randomized_and_multicast_addresses_have_no_vendor() {
        assert_eq!(of("ba:27:eb:12:34:56"), None);
        assert_eq!(of("01:00:5e:00:00:01"), None);
    }
}
