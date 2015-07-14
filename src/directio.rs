//! Opening files with DirectIO and managing DirectIO buffers
extern crate std;
extern crate libc;

use libc::{c_void};

use std::path::Path;
use std::os::unix::io::{AsRawFd, RawFd};
use std::io;
use directio::Mode::*;
use directio::FileAccess::*;

use super::FD;
use aligned::AlignedBuf;

pub struct DirectFile {
    fd: FD,
    alignment: usize,
}

const O_DIRECT: i32 = 0x4000;   // Linux

#[inline]
fn retry<F: Fn() -> isize>(f: F) -> isize {
    loop {
        let n = f();
        if n != -1 || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return n
        }
    }
}

pub enum Mode {
    Open,
    Append,
    Truncate,
}

pub enum FileAccess {
    Read,
    Write,
    ReadWrite,
}

impl DirectFile {
    // XXX auto-query directio alignment
    pub fn open<P: AsRef<Path>>(path: P, mode: Mode, fa: FileAccess, alignment: usize) -> io::Result<DirectFile> {
        let flags = O_DIRECT | match mode {
            Open => 0,
            Append => libc::O_APPEND,
            Truncate => libc::O_TRUNC,
        };
        // Opening with a write permission must silently create the file.
        let (flags, mode) = match fa {
            Read => (flags | libc::O_RDONLY, 0),
            Write => (flags | libc::O_WRONLY | libc::O_CREAT,
                      libc::S_IRUSR | libc::S_IWUSR),
            ReadWrite => (flags | libc::O_RDWR | libc::O_CREAT,
                          libc::S_IRUSR | libc::S_IWUSR),
        };

        let path = path.as_ref().as_os_str().to_bytes().unwrap();
        match retry(|| unsafe { libc::open(path.as_ptr() as *const i8, flags, mode) as isize }) {
            -1 => Err(io::Error::last_os_error()),
            fd => Ok(DirectFile { fd: FD(fd as i32), alignment: alignment }),
        }
    }

    pub fn alignment(&self) -> usize { self.alignment }

    pub fn pread(&self, buf: &mut AlignedBuf, off: u64) -> io::Result<usize> {
        let r = unsafe { ::libc::pread(self.fd.as_raw_fd(), buf.as_mut_ptr() as *mut c_void, buf.len() as u64, off as i64) };

        if r < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(r as usize)
        }
    }

    pub fn pwrite(&self, buf: &AlignedBuf, off: u64) -> io::Result<usize> {
        let r = unsafe {
            ::libc::pwrite(self.fd.as_raw_fd(),
                           buf.as_ptr() as *const c_void,
                           buf.len() as u64,
                           off as i64) };

        if r < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(r as usize)
        }
    }
}

impl AsRawFd for DirectFile {
    fn as_raw_fd(&self) -> RawFd { self.fd.as_raw_fd() }
}

#[cfg(test)]
mod test {
    extern crate tempdir;
    
    use std::path::Path;
    use super::DirectFile;
    use super::Mode::*;
    use super::FileAccess::*;
    use aligned::AlignedBuf;
    use self::tempdir::TempDir;
    
    fn tmpfile(name: &str) -> DirectFile {
        let tmp = TempDir::new_in(&Path::new("."), "test").unwrap();
        let mut path = tmp.into_path();

        path.push(name);
        DirectFile::open(&path, Truncate, ReadWrite, 4096).unwrap()
    }

    #[test]
    fn simple() {
        let file = tmpfile("direct");
        let data = match AlignedBuf::from_slice(&['x' as u8; 4096][..], 4096) {
            None => panic!("buf alloc"),
            Some(b) => b
        };
        
        match file.pwrite(&data, 0) {
            Ok(_) => (),
            Err(e) => panic!("write error {}", e)
        }
    }
}
