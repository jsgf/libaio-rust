#![allow(dead_code)]
extern crate std;
extern crate libc;

use libc::{uint16_t, uint32_t, uint64_t, int64_t, c_long, c_int, size_t};
pub use libc::timespec;
use std::mem::zeroed;
use std::default::Default;

// Taken from linux/include/uabi/linux/aio_abi.h
// This is a kernel ABI, so there should be no need to worry about it changing.
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct Struct_iocb {
    pub data: uint64_t,             // ends up in io_event.data

    pub key: uint32_t,
    pub aio_reserved1: uint32_t,

    pub aio_lio_opcode: uint16_t,
    pub aio_reqprio: uint16_t,
    pub aio_fildes: uint32_t,

    // PREAD/PWRITE -> void *
    // PREADV/PWRITEV -> iovec
    pub aio_buf: uint64_t,
    pub aio_count: uint64_t,        // bytes or iovec entries
    pub aio_offset: uint64_t,

    pub aio_reserved2: uint64_t,

    pub aio_flags: uint32_t,

    pub aio_resfd: uint32_t,
}

impl Default for Struct_iocb {
    fn default() -> Struct_iocb {
        Struct_iocb { aio_lio_opcode: Iocmd::IO_CMD_NOOP as u16,
                      aio_fildes: -1 as u32,
                      .. unsafe { zeroed() }
        }
    }
}

#[repr(C)]
pub enum Iocmd {
    IO_CMD_PREAD = 0,
    IO_CMD_PWRITE = 1,
    IO_CMD_FSYNC = 2,
    IO_CMD_FDSYNC = 3,
    // IOCB_CMD_PREADX = 4,
    // IOCB_CMD_POLL = 5,
    IO_CMD_NOOP = 6,
    IO_CMD_PREADV = 7,
    IO_CMD_PWRITEV = 8,
}

pub const IOCB_FLAG_RESFD : u32 = 1 << 0;

#[repr(C)]
#[allow(non_camel_case_types)]
pub struct Struct_io_event {
    pub data: uint64_t,
    pub obj: uint64_t,
    pub res: int64_t,
    pub res2: int64_t,
}

impl Default for Struct_io_event {
    fn default() -> Struct_io_event {
        unsafe { zeroed() }
    }
}

#[allow(non_camel_case_types)]
pub enum Struct_io_context { }
#[allow(non_camel_case_types)]
pub type io_context_t = *mut Struct_io_context;

#[repr(C)]
pub struct Struct_iovec {
    pub iov_base: *mut u8,
    pub iov_len: size_t,
}

#[link(name = "aio")]
extern "C" {
    pub fn io_queue_init(maxevents: c_int, ctxp: *mut io_context_t) -> c_int;
    pub fn io_queue_release(ctx: io_context_t) -> c_int;
    pub fn io_queue_run(ctx: io_context_t) -> c_int;
    pub fn io_setup(maxevents: c_int, ctxp: *mut io_context_t) -> c_int;
    pub fn io_destroy(ctx: io_context_t) -> c_int;
    pub fn io_submit(ctx: io_context_t, nr: c_long, ios: *mut *mut Struct_iocb) -> c_int;
    pub fn io_cancel(ctx: io_context_t, iocb: *mut Struct_iocb, evt: *mut Struct_io_event) -> c_int;
    pub fn io_getevents(ctx_id: io_context_t, min_nr: c_long,
                        nr: c_long, events: *mut Struct_io_event,
                        timeout: *mut timespec) -> c_int;
}

#[cfg(test)]
mod test {
    use std::mem::size_of;

    #[test]
    fn test_sizes() {
        // Check against kernel ABI
        assert!(size_of::<super::Struct_io_event>() == 32);
        assert!(size_of::<super::Struct_iocb>() == 64);
    }
}
