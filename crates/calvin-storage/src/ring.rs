use memmap2::{MmapMut, MmapOptions};
use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

pub const RING_HEADER_MAGIC: u32 = 0x51000001;

#[repr(C)]
pub struct RingHeader {
    pub magic: u32,
    pub version: u32,
    pub capacity: u64,
    pub write_idx: AtomicU64,
    pub read_idx: AtomicU64,
}

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
        let cap = header.capacity;
        let len = bytes.len() as u64;

        if len > cap {
            return false;
        }

        loop {
            let r = header.read_idx.load(Ordering::Acquire);
            let w = header.write_idx.load(Ordering::Acquire);

            let available = cap - (w.wrapping_sub(r));
            if len <= available {
                let offset = w % cap;
                let data_slice =
                    unsafe { std::slice::from_raw_parts_mut(self.data.as_ptr(), cap as usize) };

                let first_part = std::cmp::min(len, cap - offset);
                data_slice[(offset as usize)..((offset + first_part) as usize)]
                    .copy_from_slice(&bytes[..(first_part as usize)]);

                if first_part < len {
                    let second_part = len - first_part;
                    data_slice[..(second_part as usize)]
                        .copy_from_slice(&bytes[(first_part as usize)..]);
                }

                header
                    .write_idx
                    .store(w.wrapping_add(len), Ordering::Release);
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
        let cap = header.capacity;

        let r = header.read_idx.load(Ordering::Acquire);
        let w = header.write_idx.load(Ordering::Acquire);

        let available = w.wrapping_sub(r);
        if available == 0 {
            return 0;
        }

        let to_read = std::cmp::min(available, out.len() as u64);
        let offset = r % cap;
        let data_slice = unsafe { std::slice::from_raw_parts(self.data.as_ptr(), cap as usize) };

        let first_part = std::cmp::min(to_read, cap - offset);
        out[..(first_part as usize)]
            .copy_from_slice(&data_slice[(offset as usize)..((offset + first_part) as usize)]);

        if first_part < to_read {
            let second_part = to_read - first_part;
            out[(first_part as usize)..].copy_from_slice(&data_slice[..(second_part as usize)]);
        }

        header
            .read_idx
            .store(r.wrapping_add(to_read), Ordering::Release);
        to_read as usize
    }
}
