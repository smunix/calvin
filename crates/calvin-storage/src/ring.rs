use memmap2::{MmapMut, MmapOptions};
use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

pub const RING_HEADER_MAGIC: u32 = 0x51000001;

/// Value Object representing the shared-memory ring buffer magic identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RingMagic(pub u32);

impl RingMagic {
    pub const DEFAULT: Self = Self(RING_HEADER_MAGIC);

    pub const fn is_valid(self) -> bool {
        self.0 == RING_HEADER_MAGIC
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl Default for RingMagic {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Value Object representing the ring buffer protocol/layout version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RingVersion(pub u32);

impl RingVersion {
    pub const V1: Self = Self(1);

    pub const fn is_supported(self) -> bool {
        self.0 == 1
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl Default for RingVersion {
    fn default() -> Self {
        Self::V1
    }
}

/// Value Object representing the validated storage capacity of a ring buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RingCapacity(pub u64);

impl RingCapacity {
    pub fn new(capacity: u64) -> io::Result<Self> {
        if capacity == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Ring capacity must be greater than zero",
            ));
        }
        Ok(Self(capacity))
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

impl From<u64> for RingCapacity {
    fn from(cap: u64) -> Self {
        Self(cap)
    }
}

impl From<RingCapacity> for u64 {
    fn from(cap: RingCapacity) -> Self {
        cap.0
    }
}

/// Aggregate descriptor for shared-memory ring buffer headers.
#[repr(C)]
pub struct RingHeader {
    pub magic: u32,
    pub version: u32,
    pub capacity: u64,
    pub write_idx: AtomicU64,
    pub read_idx: AtomicU64,
}

impl RingHeader {
    pub fn ring_magic(&self) -> RingMagic {
        RingMagic(self.magic)
    }

    pub fn ring_version(&self) -> RingVersion {
        RingVersion(self.version)
    }

    pub fn ring_capacity(&self) -> RingCapacity {
        RingCapacity(self.capacity)
    }

    pub fn is_valid(&self) -> bool {
        self.ring_magic().is_valid() && self.ring_version().is_supported()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QoS {
    Reliable,
    Unreliable,
}

pub struct ShmRing {
    _mmap: MmapMut,
    header: NonNull<RingHeader>,
    data: NonNull<u8>,
}

unsafe impl Send for ShmRing {}
unsafe impl Sync for ShmRing {}

impl ShmRing {
    pub fn create<P: AsRef<Path>>(path: P, capacity: u64) -> io::Result<Self> {
        let total_size = std::mem::size_of::<RingHeader>() as u64 + capacity;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        file.set_len(total_size)?;

        let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

        let header_ptr = mmap.as_mut_ptr() as *mut RingHeader;
        unsafe {
            header_ptr.write(RingHeader {
                magic: RING_HEADER_MAGIC,
                version: 1,
                capacity,
                write_idx: AtomicU64::new(0),
                read_idx: AtomicU64::new(0),
            });
        }

        let data_ptr = unsafe { mmap.as_mut_ptr().add(std::mem::size_of::<RingHeader>()) };

        Ok(Self {
            _mmap: mmap,

            header: NonNull::new(header_ptr).unwrap(),
            data: NonNull::new(data_ptr).unwrap(),
        })
    }

    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut mmap = unsafe { MmapOptions::new().map_mut(&file)? };

        if mmap.len() < std::mem::size_of::<RingHeader>() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "File too small for ring header",
            ));
        }

        let header_ptr = mmap.as_mut_ptr() as *mut RingHeader;
        let magic = unsafe { (*header_ptr).magic };
        if magic != RING_HEADER_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid ring magic",
            ));
        }

        let data_ptr = unsafe { mmap.as_mut_ptr().add(std::mem::size_of::<RingHeader>()) };

        Ok(Self {
            _mmap: mmap,

            header: NonNull::new(header_ptr).unwrap(),
            data: NonNull::new(data_ptr).unwrap(),
        })
    }

    pub fn capacity(&self) -> u64 {
        unsafe { self.header.as_ref().capacity }
    }

    pub fn push(&self, bytes: &[u8], qos: QoS) -> bool {
        let header = unsafe { self.header.as_ref() };
        let capacity = header.capacity;
        let bytes_len = bytes.len() as u64;

        if bytes_len > capacity {
            return false;
        }

        loop {
            let read_pos = header.read_idx.load(Ordering::Acquire);
            let write_pos = header.write_idx.load(Ordering::Acquire);

            let available = capacity - write_pos.wrapping_sub(read_pos);
            if bytes_len <= available {
                let offset = write_pos % capacity;
                let data_slice = unsafe {
                    std::slice::from_raw_parts_mut(self.data.as_ptr(), capacity as usize)
                };

                let first_part = std::cmp::min(bytes_len, capacity - offset);
                data_slice[(offset as usize)..((offset + first_part) as usize)]
                    .copy_from_slice(&bytes[..(first_part as usize)]);

                if first_part < bytes_len {
                    let second_part = bytes_len - first_part;
                    data_slice[..(second_part as usize)]
                        .copy_from_slice(&bytes[(first_part as usize)..]);
                }

                header
                    .write_idx
                    .store(write_pos.wrapping_add(bytes_len), Ordering::Release);
                return true;
            }

            match qos {
                QoS::Reliable => std::hint::spin_loop(), // block until space
                QoS::Unreliable => return false,         // drop
            }
        }
    }

    pub fn pop(&self, out: &mut [u8]) -> usize {
        let header = unsafe { self.header.as_ref() };
        let capacity = header.capacity;

        let read_pos = header.read_idx.load(Ordering::Acquire);
        let write_pos = header.write_idx.load(Ordering::Acquire);

        let available = write_pos.wrapping_sub(read_pos);
        if available == 0 {
            return 0;
        }

        let to_read = std::cmp::min(available, out.len() as u64);
        let offset = read_pos % capacity;
        let data_slice =
            unsafe { std::slice::from_raw_parts(self.data.as_ptr(), capacity as usize) };

        let first_part = std::cmp::min(to_read, capacity - offset);
        out[..(first_part as usize)]
            .copy_from_slice(&data_slice[(offset as usize)..((offset + first_part) as usize)]);

        if first_part < to_read {
            let second_part = to_read - first_part;
            out[(first_part as usize)..].copy_from_slice(&data_slice[..(second_part as usize)]);
        }

        header
            .read_idx
            .store(read_pos.wrapping_add(to_read), Ordering::Release);
        to_read as usize
    }
}
