//! Opening files with DirectIO and managing DirectIO buffers
extern crate std;
extern crate libc;

use libc::{c_void};

use std::num::{Int, SignedInt};
use std::path::Path;
use std::io::{FileMode, FileAccess};
use std::io::{IoResult, IoError, Open, Append, Truncate, Read, Write, ReadWrite};
use std::os::unix::{AsRawFd, Fd};

use super::FD;
use aligned::AlignedBuf;

pub struct DirectFile(FD, uint);

const O_DIRECT: i32 = 0x4000;   // Linux

#[inline]
fn retry<T: SignedInt> (f: || -> T) -> T {
    let one: T = Int::one();
    loop {
        let n = f();
        if n == -one && std::os::errno() == libc::EINTR as uint { }
        else { return n }
    }
}

impl DirectFile {
    // XXX auto-query directio alignment
    pub fn open(path: &Path, fm: FileMode, fa: FileAccess, alignment: uint) -> IoResult<DirectFile> {
        let flags = O_DIRECT | match fm {
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

        let path = path.to_c_str();
        match retry(|| unsafe { libc::open(path.as_ptr(), flags, mode) }) {
            -1 => Err(IoError::last_error()),
            fd => Ok(DirectFile(FD(fd), alignment)),
        }
    }

    pub fn alignment(&self) -> uint { let DirectFile(_, a) = *self; a }

    pub fn pread(&self, buf: &mut AlignedBuf, off: u64) -> IoResult<uint> {
        let DirectFile(fd, _) = *self;
        let r = unsafe { ::libc::pread(fd.as_raw_fd(), buf.as_mut_ptr() as *mut c_void, buf.len() as u64, off as i64) };

        if r < 0 {
            Err(IoError::last_error())
        } else {
            Ok(r as uint)
        }
    }

    pub fn pwrite(&self, buf: &AlignedBuf, off: u64) -> IoResult<uint> {
        let DirectFile(fd, _) = *self;
        let r = unsafe { ::libc::pwrite(fd.as_raw_fd(), buf.as_slice().as_ptr() as *const c_void, buf.len() as u64, off as i64) };

        if r < 0 {
            Err(IoError::last_error())
        } else {
            Ok(r as uint)
        }
    }
}

impl AsRawFd for DirectFile {
    fn as_raw_fd(&self) -> Fd {
        let DirectFile(fd, _) = *self;
        fd.as_raw_fd()
    }
}

#[cfg(test)]
mod test {
    use std::io::{TempDir, Truncate, ReadWrite};
    use std::path::Path;
    use super::DirectFile;
    use aligned::AlignedBuf;
    
    fn tmpfile(name: &str) -> DirectFile {
        let tmp = TempDir::new_in(&Path::new("."), "test").unwrap();
        let mut path = tmp.path().clone();

        path.push(name);
        match DirectFile::open(&path, Truncate, ReadWrite, 4096) {
            Err(e) => panic!("open failed {}", e),
            Ok(f) => f
        }
    }

    #[test]
    fn simple() {
        let file = tmpfile("direct");
        let data = match AlignedBuf::from_slice(['x' as u8, ..4096].as_slice(), 4096) {
            None => panic!("buf alloc"),
            Some(b) => b
        };
        
        match file.pwrite(&data, 0) {
            Ok(_) => (),
            Err(e) => panic!("write error {}", e)
        }
    }
}
