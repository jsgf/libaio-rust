//! Put AIO results into a Future
//!
//! This module represents pending AIO as a Future of the IO result.
extern crate std;

use std::comm;
use std::sync::Future;
use std::io::IoResult;
use std::os::unix::AsRawFd;

use buf::{RdBuf, WrBuf};
use raw;
use raw::IoOp;
use super::eagain;

enum IoFut<Wb: WrBuf, Rb: RdBuf> {
    Pread(SyncSender<(IoResult<uint>, Rb)>),
    Preadv(SyncSender<(IoResult<uint>, Vec<Rb>)>),
    Pwrite(SyncSender<(IoResult<uint>, Wb)>),
    Pwritev(SyncSender<(IoResult<uint>, Vec<Wb>)>),
    Fsync(SyncSender<IoResult<()>>),
}

type RawIoctx<Wb, Rb> = raw::Iocontext<IoFut<Wb, Rb>, Wb, Rb>;

pub struct Iocontext<Wb: WrBuf + Send, Rb: RdBuf + Send> {
    ctx: RawIoctx<Wb, Rb>,
}

impl<Wb: WrBuf + Send, Rb: RdBuf + Send> Iocontext<Wb, Rb> {
    /// Construct a new Iocontext.
    pub fn new(max: uint) -> IoResult<Iocontext<Wb, Rb>> {
        Ok(Iocontext { ctx: try!(raw::Iocontext::new(max)) })
    }

    fn results(&mut self) {
        let max = self.ctx.maxops();
        match self.ctx.results(1, max, None) {
            Err(e) => panic!("get results failed {}", e),
            Ok(res) =>
                for (op, ores) in res.into_iter() {
                    match op {
                        IoOp::Noop => (),

                        IoOp::Pread(buf, IoFut::Pread(tx)) => tx.send((ores, buf)),
                        IoOp::Pread(_, _) => panic!("badness Pread"),

                        IoOp::Preadv(buf, IoFut::Preadv(tx)) => tx.send((ores, buf)),
                        IoOp::Preadv(_, _) => panic!("badness Preadv"),

                        IoOp::Pwrite(buf, IoFut::Pwrite(tx)) => tx.send((ores, buf)),
                        IoOp::Pwrite(_, _) => panic!("badness Pwrite"),

                        IoOp::Pwritev(buf, IoFut::Pwritev(tx)) => tx.send((ores, buf)),
                        IoOp::Pwritev(_, _) => panic!("badness Pwritev"),

                        IoOp::Fsync(IoFut::Fsync(tx)) => tx.send(ores.map(|_| ())),
                        IoOp::Fdsync(IoFut::Fsync(tx)) => tx.send(ores.map(|_| ())),
                        IoOp::Fsync(_) | IoOp::Fdsync(_) => panic!("badness fsync"),
                    }
                }
        }
    }

    /// Submit all pending IO operations.
    pub fn flush(&mut self) -> IoResult<()> {
        match self.ctx.submit() {
            Err(e) => return Err(e),
            Ok(_) => (),
        };

        while self.ctx.submitted() > 0 {
            self.results();
        }

        Ok(())
    }

    /// Submit a pread operation.
    pub fn pread<F: AsRawFd>(&mut self, file: &F, buf: Rb, off: u64) -> Future<(IoResult<uint>, Rb)> {
        let (tx, rx) = comm::sync_channel(1);

        match self.ctx.pread(file, buf, off, IoFut::Pread(tx)) {
            Ok(()) => Future::from_receiver(rx),
            Err((buf, _)) => Future::from_value((Err(eagain()), buf)),
        }
    }

    /// Submit a preadv operation.
    pub fn preadv<F: AsRawFd>(&mut self, file: &F, bufv: Vec<Rb>, off: u64) -> Future<(IoResult<uint>, Vec<Rb>)> {
        let (tx, rx) = comm::sync_channel(1);

        match self.ctx.preadv(file, bufv, off, IoFut::Preadv(tx)) {
            Ok(()) => Future::from_receiver(rx),
            Err((bufv, _)) => Future::from_value((Err(eagain()), bufv)),
        }
    }

    /// Submit a pwrite operation.
    pub fn pwrite<F: AsRawFd>(&mut self, file: &F, buf: Wb, off: u64) -> Future<(IoResult<uint>, Wb)> {
        let (tx, rx) = comm::sync_channel(1);

        match self.ctx.pwrite(file, buf, off, IoFut::Pwrite(tx)) {
            Ok(()) => Future::from_receiver(rx),
            Err((buf, _)) => Future::from_value((Err(eagain()), buf)),
        }
    }

    /// Submit a pwritev operation.
    pub fn pwritev<F: AsRawFd>(&mut self, file: &F, bufv: Vec<Wb>, off: u64) -> Future<(IoResult<uint>, Vec<Wb>)> {
        let (tx, rx) = comm::sync_channel(1);

        match self.ctx.pwritev(file, bufv, off, IoFut::Pwritev(tx)) {
            Ok(()) => Future::from_receiver(rx),
            Err((bufv, _)) => Future::from_value((Err(eagain()), bufv)),
        }
    }

    /// Submit an fsync.
    pub fn fsync<F: AsRawFd>(&mut self, file: &F) -> Future<IoResult<()>> {
        let (tx, rx) = comm::sync_channel(1);

        match self.ctx.fsync(file, IoFut::Fsync(tx)) {
            Ok(()) => Future::from_receiver(rx),
            Err(_) => Future::from_value(Err(eagain())),
        }
    }

    /// Submit an fdatasync.
    pub fn fdsync<F: AsRawFd>(&mut self, file: &F) -> Future<IoResult<()>> {
        let (tx, rx) = comm::sync_channel(1);

        match self.ctx.fdsync(file, IoFut::Fsync(tx)) {
            Ok(()) => Future::from_receiver(rx),
            Err(_) => Future::from_value(Err(eagain())),
        }
    }
}

#[cfg(test)]
mod test {
    use super::Iocontext;

    use std::io::{TempDir, File, Truncate, ReadWrite};

    fn tmpfile(name: &str) -> File {
        let tmp = TempDir::new("test").unwrap();
        let mut path = tmp.path().clone();

        path.push(name);
        File::open_mode(&path, Truncate, ReadWrite).unwrap()
    }

    #[test]
    fn simple() {
        let mut io = match Iocontext::new(10) {
            Err(e) => panic!("new failed {}", e),
            Ok(t) => t,
        };
        let file = tmpfile("chan");

        let wbuf = Vec::from_fn(40, |_| 'x' as u8);
        let rbuf = Vec::from_fn(100, |_| 0 as u8);

        let w = io.pwrite(&file, wbuf, 0);
        let r = io.pread(&file, rbuf, 0);

        assert!(io.flush().is_ok());

        let wb2 = match w.into_inner() {
            (Ok(sz), wb) => { assert_eq!(sz, 40); wb },
            (Err(e), _) => panic!("write failed {}", e),
        };

        match r.into_inner() {
            (Ok(sz), rb) => { assert_eq!(sz, 40); assert_eq!(rb[0 .. 40], wb2.as_slice()) },
            (Err(e), _) => panic!("read failed {}", e),
        }        
    }
}
