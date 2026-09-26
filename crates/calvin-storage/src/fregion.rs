use memmap2::{MmapMut, MmapOptions};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub const HOBBES_FREGION_MAGIC: u32 = 0x10a1db0d;
pub const PAGE_SIZE: usize = 4096;

#[repr(C)]
#[derive()]
pub struct RegionHeader {
    pub magic: u32,
    pub version: u32,
    pub size: u64,
    pub root_offset: u64,
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
