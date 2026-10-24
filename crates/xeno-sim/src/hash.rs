//! A stable hash of the sim's state: 64-bit FNV-1a over fields written one by one, in
//! a fixed order, as little-endian bytes. It does not use `std::hash`, whose output
//! may change between Rust versions, so a hash recorded on one machine or toolchain
//! can be compared on another.

const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// Collects the state's bytes.
#[derive(Debug, Clone)]
pub struct StateHasher(u64);

impl Default for StateHasher {
    fn default() -> Self {
        StateHasher(OFFSET)
    }
}

impl StateHasher {
    pub fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(PRIME);
        }
    }

    pub fn u8(&mut self, n: u8) {
        self.bytes(&[n]);
    }

    pub fn u16(&mut self, n: u16) {
        self.bytes(&n.to_le_bytes());
    }

    pub fn i32(&mut self, n: i32) {
        self.bytes(&n.to_le_bytes());
    }

    pub fn u64(&mut self, n: u64) {
        self.bytes(&n.to_le_bytes());
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

/// A part of the state that writes itself into a [`StateHasher`]. Every field that
/// can change goes in, collections with their length first.
pub trait StateHash {
    fn state_hash(&self, h: &mut StateHasher);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_is_fnv_1a() {
        let hash = |bytes: &[u8]| {
            let mut h = StateHasher::default();
            h.bytes(bytes);
            h.finish()
        };
        // The published FNV-1a 64 test vectors.
        assert_eq!(hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(hash(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(hash(b"foobar"), 0x8594_4171_f739_67e8);
        let mut h = StateHasher::default();
        h.u16(0x6261);
        assert_eq!(h.finish(), hash(b"ab"));
    }
}
