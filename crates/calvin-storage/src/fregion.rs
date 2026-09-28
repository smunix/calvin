use memmap2::{MmapMut, MmapOptions};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub const HOBBES_FREGION_MAGIC: u32 = 0x10a1db0d;
pub const PAGE_SIZE: usize = 4096;

/// Value Object representing the file region magic identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegionMagic(pub u32);

impl RegionMagic {
    pub const DEFAULT: Self = Self(HOBBES_FREGION_MAGIC);

    pub const fn is_valid(self) -> bool {
        self.0 == HOBBES_FREGION_MAGIC
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl Default for RegionMagic {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Value Object representing the file region format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegionVersion(pub u32);

impl RegionVersion {
    pub const V1: Self = Self(1);

    pub const fn is_supported(self) -> bool {
        self.0 == 1
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl Default for RegionVersion {
    fn default() -> Self {
        Self::V1
    }
}

/// Value Object representing the aligned size of a memory-mapped file region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegionSize(pub u64);

impl RegionSize {
    pub fn new(size: u64) -> io::Result<Self> {
        if size == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Region size must be greater than zero",
            ));
        }
        Ok(Self(size))
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

/// Value Object representing an offset within a structured file region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RootOffset(pub u64);

impl RootOffset {
    pub const ZERO: Self = Self(0);

    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionHeader {
    pub magic: u32,
    pub version: u32,
    pub size: u64,
    pub root_offset: u64,
}

impl RegionHeader {
    pub fn region_magic(&self) -> RegionMagic {
        RegionMagic(self.magic)
    }

    pub fn region_version(&self) -> RegionVersion {
        RegionVersion(self.version)
    }

    pub fn region_size(&self) -> RegionSize {
        RegionSize(self.size)
    }

    pub fn root_offset(&self) -> RootOffset {
        RootOffset(self.root_offset)
    }

    pub fn is_valid(&self) -> bool {
        self.region_magic().is_valid() && self.region_version().is_supported()
    }
}

pub struct FRegion {
    _file: File,
    mmap: MmapMut,
}

impl FRegion {
    pub fn create<P: AsRef<Path>>(path: P, initial_size: usize) -> io::Result<Self> {
        let size = (initial_size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1); // align
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        file.set_len(size as u64)?;

        let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

        // Write header
        let header = RegionHeader {
            magic: HOBBES_FREGION_MAGIC,
            version: 1,
            size: size as u64,
            root_offset: 0,
        };

        unsafe {
            let ptr = mmap.as_mut_ptr() as *mut RegionHeader;
            *ptr = header;
        }

        Ok(Self { _file: file, mmap })
    }

    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let mmap = unsafe { MmapOptions::new().map_mut(&file)? };

        if mmap.len() < std::mem::size_of::<RegionHeader>() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "File too small"));
        }

        let header = unsafe { &*(mmap.as_ptr() as *const RegionHeader) };
        if header.magic != HOBBES_FREGION_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid magic number",
            ));
        }

        Ok(Self { _file: file, mmap })
    }

    pub fn header(&self) -> &RegionHeader {
        unsafe { &*(self.mmap.as_ptr() as *const RegionHeader) }
    }
}
