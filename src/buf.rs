extern crate std;

/// Trait for types implementing a read buffer.
pub trait RdBuf {
    /// Return a mutable u8 slice into some storage which need not be initialized.
    fn rdbuf<'a>(&'a mut self) -> &'a mut [u8];

    /// Called to indicate some range of the buffer was updated by the read, from [`base` .. `base`+`len`).
    fn rdupdate(&mut self, _base: usize, _len: usize) {}
}

/// Trait for types implementing a write buffer.
pub trait WrBuf {
    /// Return an initialized immutable slice which is the source data for a write.
    fn wrbuf<'a>(&'a self) -> &'a [u8];
}

/// Wrapper for plain [u8] implementing RdBuf and WrBuf traits.
pub type Buf<'b> = &'b mut [u8];

impl<'b> RdBuf for Buf<'b> {
    fn rdbuf(&mut self) -> &mut [u8] { *self }
}

impl<'b> WrBuf for Buf<'b> {
    fn wrbuf(&self) -> &[u8] { *self }
}

impl RdBuf for Vec<u8> {
    /// Returns slice of `Vec<u8>`'s capacity - not its current size
    fn rdbuf(&mut self) -> &mut [u8] {
        let p = self.as_mut_ptr();
        unsafe { std::slice::from_raw_parts_mut(p, self.capacity()) }
    }

    /// Update the size of the vector to match initialized parts; assumes all bytes are written to the last
    fn rdupdate(&mut self, base: usize, len: usize) {
        unsafe { self.set_len(base + len); }
    }
}

impl WrBuf for Vec<u8> {
    fn wrbuf(&self) -> &[u8] { self.as_slice() }
}
/*
impl<T : RdBuf> RdBuf for Box<T> {
    fn rdbuf(&mut self) -> &mut [u8] { (*self).rdbuf() }
}

impl<T : WrBuf> WrBuf for Box<T> {
    fn wrbuf(&self) -> &[u8] { (*self).wrbuf() }
}
*/
