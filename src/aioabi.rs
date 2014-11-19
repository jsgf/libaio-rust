extern crate libc;

use libc::{uint16_t, uint32_t, uint64_t, int64_t, c_void, c_long, c_int, size_t};
use std::mem::{transmute, zeroed};
use std::default::Default;

// Taken from linux/include/uabi/linux/aio_abi.h
// This is a kernel ABI, so there should be no need to worry about it changing.
#[repr(C)]
#[allow(non_camel_case_types)]
#[allow(dead_code)]
pub struct Struct_iocb {
    data: uint64_t,             // ends up in io_event.data

    key: uint32_t,
    aio_reserved1: uint32_t,

    aio_lio_opcode: uint16_t,
    aio_reqprio: uint16_t,
    aio_fildes: uint32_t,

    // must be 64 bits, so this assumes 64-bit build
    // PREAD/PWRITE -> void *
    // PREADV/PWRITEV -> iovec
    aio_buf: uint64_t,
    aio_count: uint64_t,        // bytes or iovec entries
    aio_offset: uint64_t,

    aio_reserved2: uint64_t,

    aio_flags: uint32_t,

    aio_resfd: uint32_t,
}

impl Default for Struct_iocb {
    fn default() -> Struct_iocb {
        unsafe { zeroed() }
    }
}

impl Struct_iocb {
    pub fn buf_data(&mut self) -> *mut *mut c_void {
        unsafe { transmute(&self.aio_buf) }
    }
    pub fn buf_iovec(&mut self) -> *mut Struct_iovec {
        unsafe { transmute(&self.aio_buf) }
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

#[repr(C)]
#[allow(non_camel_case_types)]
#[allow(dead_code)]
pub struct Struct_io_event {
    data: uint64_t,
    obj: uint64_t,
    res: int64_t,
    res2: int64_t,
}

#[allow(non_camel_case_types)]
pub enum Struct_io_context { }
#[allow(non_camel_case_types)]
pub type io_context_t = *mut Struct_io_context;

#[repr(C)]
pub struct Struct_timespec {
    pub tv_sec: c_long,
    pub tv_nsec: c_long,
}

#[repr(C)]
pub struct Struct_iovec {
    iov_base: *mut c_void,
    iov_len: size_t,
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
                        timeout: *mut Struct_timespec) -> c_int;
}

#[cfg(test)]
mod test {
    use std::mem::{size_of_val, uninitialized};

    #[test]
    fn test_iocb_size() {
        unsafe {
            let iocb : super::Struct_iocb = uninitialized();
            assert!(size_of_val(&iocb) == 64);
        }
    }

    #[test]
    fn test_io_event_size() {
        unsafe {
            let ioev : super::Struct_io_event = uninitialized();
            assert!(size_of_val(&ioev) == 32);
        }
    }
}
