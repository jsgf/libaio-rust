extern crate std;

use std::ops::{Index,IndexMut};

enum Slot<T> {
    Free(isize),                 // Index of next entry in freelist, -1 for none
    Alloc(T),
}

/// Simple fixed size pool allocator.
pub struct Pool<T> {
    pool: Vec<Slot<T>>,
    freelist: isize,
    used: usize,
}

impl<T> Pool<T> {
    /// Create a new pool with a given size.
    pub fn new(size: usize) -> Pool<T> {
        assert!(size > 0);
        Pool { pool: (0..size).map(|i| Slot::Free((i as isize) - 1)).collect(),
               freelist: (size - 1) as isize,
               used: 0 }
    }

    /// Allocate an index in the pool. Returns None if the Pool is all used.
    pub fn allocidx(&mut self, init: T) -> Result<usize, T> {
        let idx = self.freelist;

        if idx != -1 {
            self.freelist = match self.pool[idx as usize] {
                Slot::Free(fl) => fl,
                _ => panic!("idx {} not free", idx),
            };
            self.pool[idx as usize] = Slot::Alloc(init);
            self.used += 1;
            Ok(idx as usize)
        } else {
            Err(init)
        }
    }

    /// Free an index in the pool
    pub fn freeidx(&mut self, idx: usize) -> T {
        assert!(idx < self.pool.len());
        self.freelist = idx as isize;
        self.used -= 1;
        match std::mem::replace(&mut self.pool[idx], Slot::Free(self.freelist)) {
            Slot::Alloc(v) => v,
            Slot::Free(_) => panic!("Freeing free entry {}", idx)
        }
    }

    /// Allow an entry to be freed from a raw pointer. Inherently unsafe.
    pub unsafe fn freeptr(&mut self, ptr: *const T) -> T {
        assert!(ptr as usize >= self.pool.as_ptr() as usize);
        // divide rounds down so it doesn't matter if its in the middle of Slot<>
        let idx = ((ptr as usize) - (self.pool.as_ptr() as usize)) / std::mem::size_of::<Slot<T>>();
        self.freeidx(idx)
    }

    /// Return the max number of pool entries (size passed to new()).
    #[allow(dead_code)]
    pub fn limit(&self) -> usize { self.pool.len() }

    /// Return number of currently allocated entries.
    #[allow(dead_code)]
    pub fn used(&self) -> usize { self.used }

    /// Return number of remaining unused entries.
    #[allow(dead_code)]
    pub fn avail(&self) -> usize { self.limit() - self.used() }
}

impl<T> Index<usize> for Pool<T> {
    type Output = T;
    
    fn index(&self, idx: usize) -> &T {
        match self.pool[idx] {
            Slot::Free(_) => panic!("access free index {}", idx),
            Slot::Alloc(ref t) => t
        }
    }
}

impl<T> IndexMut<usize> for Pool<T> {
    fn index_mut(&mut self, idx: usize) -> &mut T {
        match &mut self.pool[idx] {
            &mut Slot::Free(_) => panic!("access free index {}", idx),
            &mut Slot::Alloc(ref mut t) => t
        }
    }
}

#[cfg(test)]
mod test {
    use super::Pool;

    #[test]
    fn alloc() {
        let mut p = Pool::new(4);

        assert!(p.limit() == 4);
        assert!(p.used() == 0);
        assert!(p.avail() == 4);

        for i in 0..4 {
            let idx = p.allocidx(i);

            assert!(p.used() == (i + 1) as usize);
            assert!(idx.is_ok());
            assert!(p[idx.ok().unwrap()] == i);
        }

        assert!(p.avail() == 0);
        let idx = p.allocidx(10);
        assert!(p.avail() == 0);
        assert!(idx.is_err());
    }

    #[test]
    fn free() {
        let mut p = Pool::new(4);
        let mut v = Vec::new();

        assert!(p.limit() == 4);
        assert!(p.used() == 0);
        assert!(p.avail() == 4);

        for i in 0..20 {
            let idx = p.allocidx(i);

            assert!(idx.is_ok());
            assert!(idx.unwrap() < 4);
            assert!(p[idx.unwrap()] == i);

            v.push(idx.unwrap());

            if p.avail() == 0 {
                p.freeidx(v.remove(0));
                assert!(p.avail() == 1);
            }
        }
    }

    #[test]
    fn freeptr() {
        let mut p = Pool::new(4);
        let mut v = Vec::new();

        assert!(p.limit() == 4);
        assert!(p.used() == 0);
        assert!(p.avail() == 4);

        for i in 0..20 {
            let idx = p.allocidx(i);

            assert!(idx.is_ok());
            assert!(idx.ok().unwrap() < 4);
            assert!(p[idx.ok().unwrap()] == i);

            v.push(&p[idx.ok().unwrap()] as *const isize);

            if p.avail() == 0 {
                unsafe { p.freeptr(v.remove(0)) };
                assert!(p.avail() == 1);
            }
        }
    }

    #[test]
    #[should_panic]
    fn badfree1() {
        let mut p = Pool::new(4);

        let idx = p.allocidx(0);
        assert!(idx.is_ok());

        p.freeidx(idx.ok().unwrap() + 1);
    }

    #[test]
    #[should_panic]
    fn badfree2() {
        let mut p = Pool::new(4);

        let idx = p.allocidx(0);
        assert!(idx.is_ok());

        p.freeidx(idx.ok().unwrap() - 1);
    }

    #[test]
    #[should_panic]
    fn badidx0() {
        let mut p = Pool::new(4);

        p[0] = 1;
    }    

    #[test]
    #[should_panic]
    fn badidx1() {
        let mut p = Pool::new(4);

        let idx = p.allocidx(0);
        assert!(idx.is_ok());

        p[idx.ok().unwrap() + 1] = 1;
    }    

    #[test]
    #[should_panic]
    fn badidx2() {
        let mut p = Pool::new(4);

        let idx = p.allocidx(0);
        assert!(idx.is_ok());

        p[idx.ok().unwrap() - 1] = 1;
    }    

    #[test]
    #[should_panic]
    fn badptr1() {
        let mut p = Pool::new(4);
        let foo : isize = 1;

        let idx = p.allocidx(0);
        assert!(idx.is_ok());

        unsafe { p.freeptr(&foo as *const isize) };
    }    

    #[test]
    #[should_panic]
    fn badptr2() {
        let mut p = Pool::new(4);

        let idx = p.allocidx(0);
        assert!(idx.is_ok());

        unsafe {
            let ptr = ((&p[0] as *const isize as usize) - 256) as *const isize;
            p.freeptr(ptr)
        };
    }    
}
