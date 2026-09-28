pub mod fregion;
pub mod ring;

#[cfg(test)]
mod domain_tests {
    use super::fregion::{RegionHeader, RegionMagic, RegionSize, RegionVersion, RootOffset};
    use super::ring::{RingCapacity, RingHeader, RingMagic, RingVersion};
    use std::sync::atomic::AtomicU64;

    #[test]
    fn test_ring_domain_invariants() {
        assert!(RingMagic::DEFAULT.is_valid());
        assert!(!RingMagic(0x12345678).is_valid());

        assert!(RingVersion::V1.is_supported());
        assert!(!RingVersion(99).is_supported());

        assert!(RingCapacity::new(0).is_err());
        let cap = RingCapacity::new(4096).unwrap();
        assert_eq!(cap.as_u64(), 4096);

        let header = RingHeader {
            magic: RingMagic::DEFAULT.as_u32(),
            version: RingVersion::V1.as_u32(),
            capacity: 4096,
            write_idx: AtomicU64::new(0),
            read_idx: AtomicU64::new(0),
        };
        assert!(header.is_valid());
        assert_eq!(header.ring_capacity().as_u64(), 4096);
    }

    #[test]
    fn test_fregion_domain_invariants() {
        assert!(RegionMagic::DEFAULT.is_valid());
        assert!(!RegionMagic(0xdeadbeef).is_valid());

        assert!(RegionVersion::V1.is_supported());

        assert!(RegionSize::new(0).is_err());
        let size = RegionSize::new(8192).unwrap();
        assert_eq!(size.as_u64(), 8192);

        let header = RegionHeader {
            magic: RegionMagic::DEFAULT.as_u32(),
            version: RegionVersion::V1.as_u32(),
            size: 8192,
            root_offset: RootOffset::ZERO.as_u64(),
        };
        assert!(header.is_valid());
        assert_eq!(header.root_offset(), RootOffset::ZERO);
    }
}
