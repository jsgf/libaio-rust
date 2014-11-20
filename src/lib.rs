#![feature(unsafe_destructor)]

extern crate libc;

pub use buf::{RdBuf,WrBuf};
use std::os::unix::{Fd, AsRawFd};
use std::io::IoError;

mod aioabi;
mod buf;
mod pool;

pub mod raw;
pub mod chan;
pub mod future;
pub mod directio;
pub mod aligned;

/// Wrapper for a file descriptor.
struct FD(Fd);

impl FD {
    fn new<F: AsRawFd>(file: &F) -> FD { FD(file.as_raw_fd()) }
}

impl AsRawFd for FD {
    fn as_raw_fd(&self) -> Fd {
        let FD(fd) = *self;
        fd
    }
}

fn eagain() -> IoError {
    IoError::from_errno(::libc::consts::os::posix88::EAGAIN as uint, true)
}
