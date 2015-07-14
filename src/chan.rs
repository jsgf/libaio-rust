//! Channel-based interface to async IO.
//!
//! Operations can be submitted via a function call or channel, then
//! async results are returned through a result channel.
extern crate std;

use std::sync::mpsc::{Sender,SyncSender,Receiver,channel,sync_channel};
use std::io;
use std::thread;
use std::os::unix::io::AsRawFd;
use std::boxed::FnBox;
use buf::{RdBuf, WrBuf};

use super::{FD, Offset};
use raw;

fn eagain() -> io::Error {
    io::Error::from_raw_os_error(::libc::EAGAIN)
}

/// IO result.
///
/// Each operation returns an operation-specific value containing the
/// resources used by the operations and the caller's token value `T`
/// which ties the result to a specific request.
///
/// The `io::Result<usize>` typically returns the number of bytes
/// read/written on success, or an `IoError` on failure.
pub type IoRes<T, Wb, Rb> = (io::Result<usize>, raw::IoOp<T, Wb, Rb>);

/// Submit a new IO operation.
///
/// OpTx is the sender size of a channel for submitting new IO
/// operations.
type Callback<T, Wb, Rb> = Box<FnBox(&mut raw::Iocontext<T, Wb, Rb>, &Sender<IoRes<T, Wb, Rb>>)>;
type OpTx<T, Wb, Rb> = SyncSender<Callback<T,Wb,Rb>>;

/// Channel-based AIO context.
///
/// The context is simple the sending channel endpoint for submitting
/// new operations. The each operation contains all the resources it
/// needs to perform, which are returned when the operation
/// completes. The context has a few helper methods to help form
/// messages.
pub struct Iocontext<T : Send, Wb : WrBuf + Send, Rb : RdBuf + Send> {
    optx: OpTx<T, Wb, Rb>,
    resrx: Receiver<IoRes<T, Wb, Rb>>,
}

impl<T : Send, Wb : WrBuf + Send, Rb : RdBuf + Send> Iocontext<T, Wb, Rb> {
    /// Construct a new channel AIO context. When there are more than
    /// lowwater ops pending it will flush automatically; new
    /// operations will block when there's max or more outstanding
    /// operations (batched and submitted). Returns the submission and
    /// results channel endpoints.
    pub fn new(lowwater: usize, max: usize) -> io::Result<Iocontext<T, Wb, Rb>> {
        assert!(lowwater > 0 && lowwater < max);

        let mut ctx = try!(raw::Iocontext::new(max));

        // Prepare events
        let evfd = try!(ctx.get_evfd_stream());

        let (optx, oprx) = sync_channel(max); // block requests when there are too many outstanding
        let (restx, resrx) = channel();       // don't block worker - there can't be more than requests anyway

        thread::spawn(move || {
            let mut worker = ChanWorker { ctx: ctx, lowwater: lowwater };
            worker.worker(oprx, restx, evfd)
        });

        Ok(Iocontext { optx: optx, resrx: resrx })
    }

    /// Return result channel.
    ///
    /// This returns a Reciever endpoint for getting IO results, each
    /// of which is a tuple consisting of success/failure, and the
    /// return of the resources needed for the operation.
    pub fn resrx<'a>(&'a self) -> &'a Receiver<IoRes<T, Wb, Rb>> {
        &self.resrx
    }

    /// Send a flush request. This causes all pending operations to be immediately submitted.
    pub fn flush(&self) {
        self.optx.send(move |ctx: &mut raw::Iocontext<T, Wb, Rb>, _: &Sender<IoRes<T, Wb, Rb>>| {
            match ctx.submit() {
                Ok(_) => (),
                Err(_) => (),
            }
        })
    }

    fn sendhelper<F: AsRawFd>(&self, file: &F,
                              func: FnBox(&mut raw::Iocontext<T, Wb, Rb>, FD) -> Result<(), raw::IoOp<T, Wb, Rb>>) {
        let fd = FD::new(file);

        self.optx.send(move |ctx: &mut raw::Iocontext<T, Wb, Rb>, restx: &Sender<IoRes<T, Wb, Rb>>| {
            match func(ctx, fd) {
                Ok(_) => (),
                Err(r) => restx.send((Err(eagain()), r))
            }
        })
    }

    /// Send a Pread request.
    ///
    /// On success, the returned usize indicates how much of `buf` was
    /// initialized. Otherwise on error, none of it will have been.
    pub fn pread<F: AsRawFd>(&self, file: &F, buf: Rb, off: Offset, tok: T) {
        self.sendhelper(file, move |ctx, f| {
            ctx.pread(&f, buf, off, tok).map_err(|(buf, tok)| raw::IoOp::Pread(buf, tok))
        })
    }

    /// Send a Preadv request.
    ///
    /// On success, data is read into each element of `bufv` in turn.
    pub fn preadv<F: AsRawFd>(&self, file: &F, bufv: Vec<Rb>, off: Offset, tok: T) {
        self.sendhelper(file, move |ctx, f| {
            ctx.preadv(&f, bufv, off, tok).map_err(|(bufv, tok)| raw::IoOp::Preadv(bufv, tok))
        })
    }

    /// Send a Pwrite request.
    pub fn pwrite<F: AsRawFd>(&self, file: &F, buf: Wb, off: Offset, tok: T) {
        self.sendhelper(file, move |ctx, f| {
            ctx.pwrite(&f, buf, off, tok).map_err(|(buf, tok)| raw::IoOp::Pwrite(buf, tok))
        })
    }

    /// Send a Pwritev request.
    pub fn pwritev<F: AsRawFd>(&self, file: &F, bufv: Vec<Wb>, off: Offset, tok: T) {
        self.sendhelper(file, move |ctx, f| {
            ctx.pwritev(&f, bufv, off, tok).map_err(|(bufv, tok)| raw::IoOp::Pwritev(bufv, tok))
        })
    }

    /// Send a Fsync request.
    pub fn fsync<F: AsRawFd>(&self, file: &F, tok: T) {
        self.sendhelper(file, move |ctx, f| {
            ctx.fsync(&f, tok).map_err(|tok| raw::IoOp::Fsync(tok))
        })
    }

    /// Send a Fdsync request.
    pub fn fdsync<F: AsRawFd>(&self, file: &F, tok: T) {
        self.sendhelper(file, move | ctx, f| {
            ctx.fdsync(&f, tok).map_err(|tok| raw::IoOp::Fdsync(tok))
        })
    }
}

struct ChanWorker<T : Send, Wb : WrBuf + Send, Rb : RdBuf + Send> {
    ctx: raw::Iocontext<T, Wb, Rb>,

    lowwater: usize,
}

impl<T : Send, Wb : WrBuf + Send, Rb : RdBuf + Send> ChanWorker<T, Wb, Rb> {
    fn proc_results(&mut self, restx: &Sender<IoRes<T, Wb, Rb>>) {
        if self.ctx.pending() == 0 {
            return
        }

        let max = self.ctx.maxops();
        match self.ctx.results(1, max, None) {
            Err(e) => panic!("get results failed {:?}", e),
            Ok(v) =>
                for s in v.into_iter().map(|(op, res)| (res, op)) {
                    restx.send(s)
                },
        }
    }

    fn submit(&mut self) {
        match self.ctx.submit() {
            Err(e) => panic!("submit failed {:?}", e),
            Ok(_) => (),
        }
    }

    fn worker(&mut self,
              oprx: Receiver<FnBox(&mut raw::Iocontext<T, Wb, Rb>, &Sender<IoRes<T, Wb, Rb>>)>,
              restx: Sender<IoRes<T, Wb, Rb>>,
              evfd: Receiver<u64>) {
        let mut closed = false;

        while !closed || self.ctx.pending() != 0 {
            if self.ctx.batched() > self.lowwater {
                self.submit()
            }

            if closed || self.ctx.full() {
                // Don't bother with new requests (we're either
                // full-up or the input's closed), so just finish
                // things off.
                let _ = evfd.recv();
                self.proc_results(&restx)
            } else {
                // full bidirectional
                select!(
                    op = oprx.recv_opt() => match op {
                        Err(_) => { closed = true; self.submit() },
                        Ok(op) => op(&mut self.ctx, &restx),
                    },
                    _ = evfd.recv() => self.proc_results(&restx)
                );
            }
        }
    }
}

#[cfg(test)]
mod test {
    extern crate tempdir;

    use self::tempdir::TempDir;
    use std::fs::{File,OpenOptions};
    use super::Iocontext;

    fn tmpfile(name: &str) -> File {
        let tmp = TempDir::new("test").unwrap();
        let mut path = tmp.into_path();

        path.push(name);
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path).unwrap()
    }

    #[test]
    fn simple() {
        let io = match Iocontext::new(5, 10) {
            Err(e) => panic!("new failed {:?}", e),
            Ok(t) => t,
        };
        let file = tmpfile("chan");

        let wbuf = Vec::from_fn(40, |_| 'x' as u8);
        let rbuf = Vec::from_fn(100, |_| 0 as u8);
        let res = io.resrx();

        io.pread(&file, rbuf, 0, ());
        io.pwrite(&file, wbuf, 0, ());
        io.flush();

        for (res, op) in res.iter().take(2) {
            println!("res {:?} op {:?}", res, op);
        }
    }
}
