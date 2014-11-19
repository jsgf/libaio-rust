extern crate libc;

use std::io::{IoResult, IoError};
use std::default::Default;
use std::vec::Vec;
use self::aioabi as aio;

#[allow(dead_code)]
mod aioabi;

struct IoOp {
    iocb: aio::Struct_iocb,
    iov: Option<Vec<aio::Struct_iovec>>,
}

#[allow(non_camel_case_types)]
type Struct_iovec = aio::Struct_iovec;

impl Struct_iovec {
    fn new(buf: &[u8]) -> Struct_iovec {
        return Struct_iovec {iov_base: buf.as_ptr() as *mut ::libc::c_void,
                             iov_len: buf.len() as u64};
    }

/* // should be needed?
    fn new_mut(buf: &mut [u8]) -> Struct_iovec {
        return Struct_iovec {iov_base: buf.as_mut_ptr() as *mut ::libc::c_void,
                             iov_len: buf.len() as u64};
    }
*/
}

impl IoOp {
    fn pread(fd: int, buf: &mut [u8], offset: u64, data: u64) -> IoOp {
        let mut r = IoOp {iocb: aio::Struct_iocb {data: data,
                                                   aio_lio_opcode: aio::IO_CMD_PREAD as u16,
                                                   aio_fildes: fd as u32,
                                                   aio_count: buf.len() as u64,
                                                   aio_offset: offset,
                                                   ..Default::default() },
                          iov: None,
                         };
        unsafe { *r.iocb.buf_data() = buf.as_mut_ptr() as *mut ::libc::c_void };
        r
    }

    fn pwrite(fd: int, buf: &[u8], offset: u64, data: u64) -> IoOp {
        let mut r = IoOp {iocb: aio::Struct_iocb {data: data,
                                                  aio_lio_opcode: aio::IO_CMD_PWRITE as u16,
                                                  aio_fildes: fd as u32,
                                                  aio_count: buf.len() as u64,
                                                  aio_offset: offset,
                                                  ..Default::default() },
                          iov: None,
                         };
        unsafe { *r.iocb.buf_data() = buf.as_ptr() as *mut ::libc::c_void };
        r
    }

    fn pwritev(fd: int, bufs: Vec<&[u8]>, offset: u64, data: u64) -> IoOp {
        let vec = Vec::from_fn(bufs.len(), |i| Struct_iovec::new(bufs[i]));
        let vecp = vec.as_slice().as_ptr();
        let mut r = IoOp {iocb: aio::Struct_iocb {data: data,
                                                  aio_lio_opcode: aio::IO_CMD_PWRITEV as u16,
                                                  aio_fildes: fd as u32,
                                                  aio_count: bufs.len() as u64,
                                                  aio_offset: offset,
                                                  ..Default::default() },
                          iov: Some(vec),
                         };
        unsafe { *r.iocb.buf_iovec() = vecp };
        r
    }

    fn preadv(fd: int, bufs: Vec<&mut [u8]>, offset: u64, data: u64) -> IoOp {
        let vec = Vec::from_fn(bufs.len(), |i| Struct_iovec::new(bufs[i]));
        let vecp = vec.as_slice().as_ptr();
        let mut r = IoOp { iocb: aio::Struct_iocb {data: data,
                                                   aio_lio_opcode: aio::IO_CMD_PREADV as u16,
                                                   aio_fildes: fd as u32,
                                                   aio_count: bufs.len() as u64,
                                                   aio_offset: offset,
                                                   ..Default::default() },
                          iov: Some(vec),
                         };
        unsafe { *r.iocb.buf_iovec() = vecp };
        r
    }
}

pub struct Iobatch {
    iocb: Vec<*mut aio::Struct_iocb>,
    ops: Vec<IoOp>,    
}

impl Iobatch {
    fn new(maxevents: uint) -> Iobatch {
        Iobatch {iocb: Vec::with_capacity(maxevents),
                 ops: Vec::with_capacity(maxevents),}
    }

    fn submit(&mut self, ctx: &Iocontext) -> IoResult<()> {
        if self.iocb.len() == 0 {
            return Ok(())
        }

        unsafe {
            let e = aio::io_submit(ctx.ctx, self.iocb.len() as i64, self.iocb.as_mut_ptr());

            if e < 0 {
                Err(IoError::from_errno(-e as uint, true))
            } else {
                Ok(())
            }
        }
    }

    pub fn pwrite<'a>(&'a mut self, fd: int, buf: &'a [u8], offset: u64, data: u64) {
        self.ops.push(IoOp::pwrite(fd, buf, offset, data));
        let last = self.ops.len();
        assert!(last > 0);
        self.iocb.push(&mut self.ops[last-1].iocb);
    }

    pub fn pwritev<'a>(&'a mut self, fd: int, bufs: Vec<&'a [u8]>, offset: u64, data: u64) {
        self.ops.push(IoOp::pwritev(fd, bufs, offset, data));
        let last = self.ops.len();
        assert!(last > 0);
        self.iocb.push(&mut self.ops[last-1].iocb);
    }

    pub fn pread<'a>(&'a mut self, fd: int, buf: &'a mut [u8], offset: u64, data: u64) {
        self.ops.push(IoOp::pread(fd, buf, offset, data));
        let last = self.ops.len();
        assert!(last > 0);
        self.iocb.push(&mut self.ops[last-1].iocb);
    }

    pub fn preadv<'a>(&'a mut self, fd: int, bufs: Vec<&'a mut [u8]>, offset: u64, data: u64) {
        self.ops.push(IoOp::preadv(fd, bufs, offset, data));
        let last = self.ops.len();
        assert!(last > 0);
        self.iocb.push(&mut self.ops[last-1].iocb);
    }
}

pub struct Iocontext {
    ctx: aio::io_context_t,

    maxevents: uint,
}

impl Iocontext {
    pub fn new(maxevents: uint) -> IoResult<Iocontext> {
        unsafe {
            let mut r = Iocontext {ctx: (0 as aio::io_context_t),
                                   maxevents: maxevents,};
            let e = aio::io_queue_init(maxevents as ::libc::c_int, &mut r.ctx);
            
            if e != 0 {
                Err(IoError::from_errno(-e as uint, true))
            } else {
                Ok(r)
            }
        }
    }

    pub fn batch(&self) -> Iobatch {
        Iobatch::new(self.maxevents)
    }

    pub fn submit(&self, batch: &mut Iobatch) -> IoResult<()> {
        batch.submit(self)
    }
}

impl Drop for Iocontext {
    fn drop(&mut self) {
        unsafe {
            let r = aio::io_destroy(self.ctx);

            if r < 0 {
                panic!("io_destroy failed {}", IoError::from_errno(-r as uint, true));
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::Iocontext;

    #[test]
    fn init() {
        let mut ctx = match Iocontext::new(10) {
            Err(e) => panic!("error {}", e),
            Ok(ctx) => ctx,
        };

        match ctx.submit() {
            Err(e) => panic!("submit failed: {}", e),
            Ok(()) => (),
        }
    }
    
    #[test]
    fn life() {
        let mut ctx = match Iocontext::new(10) {
            Err(e) => panic!("error {}", e),
            Ok(ctx) => ctx,
        };

        {
            //let data = ['x' as u8, ..4096];
            
            // XXX should fail to compile because data's life is shorter than ctx        
            //ctx.pwrite(1, &data, 0);
        }

        match ctx.submit() {
            Err(e) => panic!("submit failed: {}", e),
            Ok(()) => (),
        }
    }
}
