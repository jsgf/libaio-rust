//! Raw interface to Async IO. This API is a thin layer on top of the
//! underlying libaio/kernel syscalls, and is intended as the building
//! block for easier to use interfaces.

extern crate std;
extern crate eventfd;

use std::io::{IoResult, IoError};
use std::fmt::Show;
use std::default::Default;
use std::os::unix::{AsRawFd};
use std::time::Duration;

use self::eventfd::EventFD;
use pool::Pool;

#[allow(dead_code)]
use aioabi as aio;

use buf::{RdBuf, WrBuf};

struct Iocontextwrap {
    ctx: aio::io_context_t,
}

/// Context for all AIO. This owns everything else, and must therefore
/// have the longest lifetime. The type parameters are:
///
/// * `T` - Every request carries a value of type T, which is returned
///     with each IO result. This allows the caller to link requests to
///     results.
///
/// * `Wb` - a write buffer type, which implements the `WrBuf` trait.
///
/// * `Rb` - a read buffer type, which implements the `RdBuf` trait.
///
/// The `Wb` and `Rb` are passed around by value, so ideally they
/// should be designed to avoid copying all the data (ie, they should
/// own an internal reference to the data).
///
/// Likewise, `T` is also copied by value, and so should be reasonably
/// small.
///
/// Each operation takes ownership of the resources it needs, then
/// returns them once the operation is complete, success or
/// failure. This allows async IO to be used safely, as the borrow
/// checker will make sure incomplete buffers are not accessible while
/// they are being used.
pub struct Iocontext<T: Send, Wb: WrBuf + Send, Rb: RdBuf + Send> {
    ctx: Iocontextwrap,         // kernel AIO context
    maxops: uint,               // max batch size

    batch: Iobatch<T, Wb, Rb>,  // next batch to be submitted

    evfd: Option<EventFD>,      // IO completion events

    submitted: uint,            // number of submitted IO operations
}


pub enum IoOp<T, Wb : WrBuf, Rb : RdBuf> {
    /// No operation - placeholder.
    Noop,

    /// Read - returns read buffer, that may be partially or completely filled.
    Pread(Rb, T),

    /// Readv - read into a vector of buffers, starting at the first.
    Preadv(Vec<Rb>, T),

    /// Write - returns the buffer used as the source for writes, unmodified.
    Pwrite(Wb, T),              // pwrite

    /// Writev - write from a vector of buffers.
    Pwritev(Vec<Wb>, T),        // pwritev

    /// Fsync. Sync a file descriptor to stable storage. Only works on
    /// some filesystems.
    Fsync(T),

    /// Fdatasync. Sync data associated with a file descriptor to
    /// disk, but not necessarily metadata (timestamps, etc). Only
    /// works on some filesystems.
    Fdsync(T),                  // fdatasync
}

fn as_mut_ptr<T>(thing: Option<&mut T>) -> *mut T {
    match thing {
        None => 0 as *mut T,
        Some(t) => t as *mut T
    }
}

#[allow(dead_code)]
fn as_ptr<T>(thing: Option<&T>) -> *const T {
    match thing {
        None => 0 as *const T,
        Some(t) => t as *const T
    }
}

fn timespec_from_duration(dur: Duration) -> aio::timespec {
    let dur = std::cmp::max(Duration::zero(), dur); // -ve time is 0

    aio::timespec { tv_sec: dur.num_seconds(), tv_nsec: dur.num_nanoseconds().unwrap() % 1_000_000_000 }
}


impl<T: Send, Wb : WrBuf + Send, Rb : RdBuf + Send> Iocontext<T, Wb, Rb> {
    /// Instantiate a new Iocontext. `maxops` is the maximum number of
    /// outstanding operations, which sets the upper limit on memory
    /// allocated.
    pub fn new(maxops: uint) -> IoResult<Iocontext<T, Wb, Rb>> {
        let mut r = Iocontext {
            ctx: Iocontextwrap { ctx: as_mut_ptr(None) },
            maxops: maxops,
            batch: Iobatch::new(maxops),
            evfd: None,
            submitted: 0,
        };
        let e = unsafe { aio::io_queue_init(maxops as i32, &mut r.ctx.ctx) };

        if e < 0 {
            Err(IoError::from_errno(e as uint, true))
        } else {
            Ok(r)
        }
    }

    // XXX how to make crate-local?
    #[doc(hidden)]
    pub fn get_evfd_stream(&mut self) -> IoResult<Receiver<u64>> {
        if self.evfd.is_none() {
            match EventFD::new(0, 0) {
                Err(e) => return Err(e),
                Ok(evfd) => self.evfd = Some(evfd),
            }

        }

        Ok(self.evfd.as_ref().unwrap().events())
    }

    /// Submit all outstanding IO operations. Returns number of submitted operations.
    pub fn submit(&mut self) -> IoResult<uint> {
        // Get the current batch and clear out the new one
        let mut iocbp = self.batch.batch();

        if iocbp.len() == 0 {
            Ok(0)
        } else {
            let r = unsafe { aio::io_submit(self.ctx.ctx, iocbp.len() as i64, iocbp.as_mut_ptr()) };

            if r < 0 {
                Err(IoError::from_errno(-r as uint, true))
            } else {
                let ru = r as uint;

                // XXX need a Vec method to remove a range
                for _ in range(0, r) {
                    if iocbp.remove(0).is_none() {
                        break;
                    }
                }
                self.submitted += ru;

                Ok(ru)
            }
        }
    }

    /// Return number of batched entries for the next submission.
    pub fn batched(&self) -> uint { self.batch.len() }

    /// Number of outstanding submitted ops.
    pub fn submitted(&self) -> uint { self.submitted }

    /// Total number of pending operations, batched and submitted.
    pub fn pending(&self) -> uint { self.batched() + self.submitted() }

    /// Return max number pending of operations.
    pub fn maxops(&self) -> uint { self.maxops }

    /// Returns true if there are already the maximum number of
    /// pending operations.
    pub fn full(&self) -> bool { self.pending() >= self.maxops }

    /// Return a vector of IO results. Each result return the `T` and
    /// buffer used for IO so the caller can use it again, and the
    /// actual result of the IO.
    pub fn results(&mut self, min: uint, max: uint, timeout: Option<Duration>) -> IoResult<Vec<(IoOp<T, Wb, Rb>, IoResult<uint>)>> {
        let mut v = Vec::from_fn(max, |_| Default::default());
        let r = unsafe {
            let mut ts = timeout.map(timespec_from_duration);
            aio::io_getevents(self.ctx.ctx, min as i64, max as i64, v.as_mut_ptr(), as_mut_ptr(ts.as_mut()))
        };

        if r < 0 {
            Err(IoError::from_errno(-r as uint, true))
        } else {
            v.truncate(r as uint);
            let mut ret = Vec::with_capacity(v.len());
            for ev in v.iter() {
                let evres = if ev.res < 0 { Err(IoError::from_errno(-ev.res as uint, true)) } else { Ok(ev.res as uint) };
                let iocb = ev.data as *mut Iocb<T, Wb, Rb>;

                self.submitted -= 1;
                ret.push((self.batch.free_iocb(iocb).op, evres));
            }
            Ok(ret)
        }
    }

    fn pack_iocb<F: AsRawFd>(&self, opcode: aio::Iocmd, file: &F, off: u64) -> aio::Struct_iocb {
        aio::Struct_iocb {
            aio_lio_opcode: opcode as u16,
            aio_fildes: file.as_raw_fd() as u32,
            aio_offset: off,
            aio_flags: self.evfd.as_ref().map_or(0, |_| aio::IOCB_FLAG_RESFD),
            aio_resfd: self.evfd.as_ref().map_or(0, |evfd| evfd.as_raw_fd() as u32),
            data: 0,

            ..Default::default()
        }
    }

    fn prep_iocb<E>(&mut self, iocb: Iocb<T, Wb, Rb>) -> Result<(), E> {
        match self.batch.alloc_iocb(iocb) {
            Err(_) => panic!("alloc failed but not full"),
            Ok(iocb) => unsafe { (*iocb).iocb.data = iocb as u64; Ok(()) },
        }
    }

    /// Queue up a pread operation.
    pub fn pread<F: AsRawFd>(&mut self, file: &F, mut buf: Rb, off: u64, tok: T) -> Result<(), (Rb, T)> {
        if self.full() {
            Err((buf, tok))
        } else {
            let bufptr = buf.rdbuf().as_ptr();
            let buflen = buf.rdbuf().len();
            let iocb = Iocb {
                iocb: aio::Struct_iocb {
                    aio_buf: bufptr as u64,
                    aio_count: buflen as u64,

                    .. self.pack_iocb(aio::Iocmd::IO_CMD_PREAD, file, off)
                },
                op: IoOp::Pread(buf, tok),
            };
            self.prep_iocb(iocb)
        }
    }
        
    /// Queue up a preadv operation.
    pub fn preadv<F: AsRawFd>(&mut self, file: &F, mut buf: Vec<Rb>, off: u64, tok: T) -> Result<(), (Vec<Rb>, T)> {
        if self.full() {
            Err((buf, tok))
        } else {
            let mut iov = Vec::from_fn(buf.len(), |b| aio::Struct_iovec { iov_base: buf[b].rdbuf().as_mut_ptr(),
                                                                          iov_len: buf[b].rdbuf().len() as u64 });
            let iocb = Iocb {
                iocb: aio::Struct_iocb {
                    aio_buf: iov.as_mut_ptr() as u64,
                    aio_count: iov.len() as u64,

                    .. self.pack_iocb(aio::Iocmd::IO_CMD_PREADV, file, off)
                },
                op: IoOp::Preadv(buf, tok),
            };
            self.prep_iocb(iocb)
        }
    }
        
    /// Queue up a pwrite operation.
    pub fn pwrite<F: AsRawFd>(&mut self, file: &F, buf: Wb, off: u64, tok: T) -> Result<(), (Wb, T)> {
        if self.full() {
            Err((buf, tok))
        } else {
            let bufptr = buf.wrbuf().as_ptr();
            let buflen = buf.wrbuf().len();
            let iocb = Iocb {
                iocb: aio::Struct_iocb {
                    aio_buf: bufptr as u64,
                    aio_count: buflen as u64,

                    .. self.pack_iocb(aio::Iocmd::IO_CMD_PWRITE, file, off)
                },
                op: IoOp::Pwrite(buf, tok),
            };
            self.prep_iocb(iocb)
        }
    }

    /// Queue up a pwritev operation.
    pub fn pwritev<F: AsRawFd>(&mut self, file: &F, bufv: Vec<Wb>, off: u64, tok: T) -> Result<(), (Vec<Wb>, T)> {
        if self.full() {
            Err((bufv, tok))
        } else {
            let iov = Vec::from_fn(bufv.len(), |b| aio::Struct_iovec { iov_base: bufv[b].wrbuf().as_ptr() as *mut u8,
                                                                       iov_len: bufv[b].wrbuf().len() as u64 });

            let iocb = Iocb {
                iocb: aio::Struct_iocb {
                    aio_buf: iov.as_ptr() as u64,
                    aio_count: iov.len() as u64,

                    .. self.pack_iocb(aio::Iocmd::IO_CMD_PWRITEV, file, off)
                },
                op: IoOp::Pwritev(bufv, tok),
            };
            self.prep_iocb(iocb)
        }
    }
        
    /// Queue up an fsync operation.
    pub fn fsync<F: AsRawFd>(&mut self, file: &F, tok: T) -> Result<(), T> {
        if self.full() {
            Err(tok)
        } else {
            let iocb = Iocb {
                iocb: self.pack_iocb(aio::Iocmd::IO_CMD_FSYNC, file, 0),
                op: IoOp::Fsync(tok),
            };
            self.prep_iocb(iocb)
        }
    }

    /// Queue up an fdsync operation.
    pub fn fdsync<F: AsRawFd>(&mut self, file: &F, tok: T) -> Result<(), T> {
        if self.full() {
            Err(tok)
        } else {
            let iocb = Iocb {
                iocb: self.pack_iocb(aio::Iocmd::IO_CMD_FDSYNC, file, 0),
                op: IoOp::Fdsync(tok),
            };
            self.prep_iocb(iocb)
        }
    }
}

impl Drop for Iocontextwrap {
    fn drop(&mut self) {
        let r = unsafe { aio::io_destroy(self.ctx) };

        if r < 0 {
            panic!("io_destroy failed {}", IoError::from_errno(-r as uint, true));
        }
    }
}

impl<T : Show, Wb : WrBuf, Rb : RdBuf> Show for IoOp<T, Wb, Rb> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        match self {
            &IoOp::Noop => write!(fmt, "Noop"),
            &IoOp::Pread(_, ref t) => write!(fmt, "Pread {}", t),
            &IoOp::Preadv(_, ref t) => write!(fmt, "Preadv {}", t),
            &IoOp::Pwrite(_, ref t) => write!(fmt, "Pwrite {}", t),
            &IoOp::Pwritev(_, ref t) => write!(fmt, "Pwritev {}", t),
            &IoOp::Fsync(ref t) => write!(fmt, "Fsync {}", t),
            &IoOp::Fdsync(ref t) => write!(fmt, "Fdsync {}", t),
        }
    }
}

struct Iocb<T, Wb : WrBuf, Rb : RdBuf> {
    iocb: aio::Struct_iocb,
    op: IoOp<T, Wb, Rb>,
}

struct Iobatch<T, Wb : WrBuf, Rb : RdBuf> {
    iocb: Pool<Iocb<T, Wb, Rb>>,                        // all iocbs
    iocbp: Vec<*mut aio::Struct_iocb>,                  // next batch
}

impl<T, Wb : WrBuf, Rb : RdBuf> Iobatch<T, Wb, Rb> {
    fn new(maxops: uint) -> Iobatch<T, Wb, Rb> {
        Iobatch {
            iocb: Pool::new(maxops),
            iocbp: Vec::with_capacity(maxops),
        }
    }

    fn len(&self) -> uint { self.iocbp.len() }

    fn batch<'a>(&'a mut self) -> &'a mut Vec<*mut aio::Struct_iocb> { &mut self.iocbp }

    // Allocate a new Iocb and also add the aio::Struct_iocb onto the current batch
    fn alloc_iocb(&mut self, init: Iocb<T, Wb, Rb>) -> Result<*mut Iocb<T, Wb, Rb>, Iocb<T, Wb, Rb>> {

        match self.iocb.allocidx(init) {
            Err(v) => Err(v),
            Ok(idx) => unsafe {
                let ptr = as_mut_ptr(Some(&mut self.iocb[idx]));
                self.iocbp.push(as_mut_ptr(Some(&mut (*ptr).iocb)));
                Ok(ptr)
            },
        }
    }

    /// Free an entry. This must not be included in the current iocbp batch.
    fn free_iocb(&mut self, iocb: *mut Iocb<T, Wb, Rb>) -> Iocb<T, Wb, Rb> {
        // XXX assert iocb is not in current self.iocbp?
        unsafe { self.iocb.freeptr(iocb as *const Iocb<T, Wb, Rb>) }
    }
}


#[cfg(test)]
mod test {
    extern crate std;
    use super::{Iocontext,Iobatch,Iocb,IoOp};
    use std::default::Default;
    use super::super::aioabi as aio;
    use std::cmp::min;
    use std::io::{TempDir, File, Truncate, ReadWrite};
    use std::time::Duration;

    #[test]
    fn batch_simple() {
        let mut b : Iobatch<uint, Vec<u8>, Vec<u8>> = Iobatch::new(100);

        match b.alloc_iocb(Iocb { iocb: aio::Struct_iocb { .. Default::default() }, op: IoOp::Noop } ) {
            Err(_) => panic!("alloc failed"),
            Ok(_) => (),
        };

        let v = b.batch();
        assert_eq!(v.len(), 1);
    }

    fn tmpfile(name: &str) -> File {
        let tmp = TempDir::new("test").unwrap();
        let mut path = tmp.path().clone();

        path.push(name);
        File::open_mode(&path, Truncate, ReadWrite).unwrap()
    }

    #[test]
    fn raw_simple() {
        #[deriving(Show)]
        enum Op {R, W};
        let mut io = match Iocontext::new(100) {
            Err(e) => panic!("iocontext new {}", e),
            Ok(io) => io
        };
        let wbuf = Vec::from_fn(40, |_| 'x' as u8);
        let rbuf = Vec::from_fn(100, |_| 0 as u8);

        assert_eq!(io.batched(), 0);
        assert_eq!(io.submitted(), 0);
        assert_eq!(io.pending(), 0);

        let file = tmpfile("foo");

        let ok = io.pwrite(&file, wbuf, 77, Op::W).is_ok();
        assert!(ok);
        assert_eq!(io.batched(), 1);
        assert_eq!(io.submitted(), 0);
        assert_eq!(io.pending(), 1);

        let ok = io.pread(&file, rbuf, 0, Op::R).is_ok();
        assert!(ok);
        assert_eq!(io.batched(), 2);
        assert_eq!(io.submitted(), 0);
        assert_eq!(io.pending(), 2);

        //assert_eq!(io.submitted(), 3);
        //assert_eq!(io.pending(), 3);

        assert_eq!(io.pending(), 2);
        assert_eq!(io.batched(), 2);
        while io.batched() > 0 {
            match io.submit() {
                Err(e) => panic!("submit failed {}", e),
                Ok(n) => assert_eq!(n, io.submitted())
            }

            match io.results(1, 10, Some(Duration::seconds(1))) {
                Err(e) => println!("results failed {}", e),
                Ok(res) => for &(ref op, ref r) in res.iter() {
                    match r {
                        &Err(ref e) => println!("{} failed {}", op, e),
                        &Ok(res) => { println!("complete {} {}", op, res);
                                      match op {
                                          &IoOp::Pread(_, Op::R) => assert_eq!(res, 100),
                                          &IoOp::Pwrite(_, Op::W) => assert_eq!(res, 40),
                                          _ => panic!("unexpected {}", op)
                                      }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn raw_writev() {
        let mut io : Iocontext<int, Vec<u8>, Vec<u8>> = match Iocontext::new(100) {
            Err(e) => panic!("iocontext new {}", e),
            Ok(io) => io
        };
        let wbufs = vec!["foo","bar","blat"].into_iter().map(|s| String::from_str(s).into_bytes()).collect();

        assert_eq!(io.batched(), 0);
        assert_eq!(io.submitted(), 0);
        assert_eq!(io.pending(), 0);

        let file = tmpfile("foov");
        let ok = io.pwritev(&file, wbufs, 0, 0i).is_ok();
        assert!(ok);

        while io.batched() > 0 {
            match io.submit() {
                Err(e) => panic!("submit failed {}", e),
                Ok(n) => assert_eq!(n, io.submitted())
            }

            match io.results(1, 10, Some(Duration::seconds(1))) {
                Err(e) => println!("results failed {}", e),
                Ok(res) => for &(ref op, ref r) in res.iter() {
                    match r {
                        &Err(ref e) => println!("{} failed {}", op, e),
                        &Ok(res) => { println!("complete {} {}", op, res);
                                      match op {
                                          &IoOp::Pwritev(_, _) => assert_eq!(res, 10),
                                          _ => panic!("unexpected {}", op)
                                      }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn raw_limit() {
        let mut io : Iocontext<uint, Vec<u8>, Vec<u8>> = match Iocontext::new(10) {
            Err(e) => panic!("iocontext new {}", e),
            Ok(io) => io
        };
        let file = tmpfile("bar");

        for i in range(0, 20) {
            let rbuf = Vec::from_fn(100, |_| 0 as u8);

            assert_eq!(min(i, 10), io.batched());
            assert_eq!(min(i, 10), io.pending());
            assert_eq!(0, io.submitted());

            let full = io.full();
            let p = io.pread(&file, rbuf, 0, 9);
            assert_eq!(i < 10, p.is_ok())
            assert_eq!(full, p.is_err());
        }
    }
}
