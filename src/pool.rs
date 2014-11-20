extern crate std;

enum Slot<T> {
    Free(int),                 // Index of next entry in freelist, -1 for none
    Alloc(T),
}

/// Simple fixed size pool allocator.
pub struct Pool<T> {
    pool: Vec<Slot<T>>,
    freelist: int,
    used: uint,
}

impl<T> Pool<T> {
    /// Create a new pool with a given size.
    pub fn new(size: uint) -> Pool<T> {
        assert!(size > 0);
        Pool { pool: Vec::from_fn(size, |i| Slot::Free((i as int) - 1)), freelist: (size - 1) as int, used: 0 }
    }

    /// Allocate an index in the pool. Returns None if the Pool is all used.
    pub fn allocidx(&mut self, init: T) -> Result<uint, T> {
        let idx = self.freelist;

        if idx != -1 {
            self.freelist = match self.pool[idx as uint] {
                Slot::Free(fl) => fl,
                _ => panic!("idx {} not free", idx),
            };
            self.pool[idx as uint] = Slot::Alloc(init);
            self.used += 1;
            Ok(idx as uint)
        } else {
            Err(init)
        }
    }

    /// Free an index in the pool
    pub fn freeidx(&mut self, idx: uint) -> T {
        assert!(idx < self.pool.len());
        self.freelist = idx as int;
        self.used -= 1;
        match std::mem::replace(&mut self.pool[idx], Slot::Free(self.freelist)) {
            Slot::Alloc(v) => v,
            Slot::Free(_) => panic!("Freeing free entry {}", idx)
        }
    }

    /// Allow an entry to be freed from a raw pointer. Inherently unsafe.
    pub unsafe fn freeptr(&mut self, ptr: *const T) -> T {
        assert!(ptr as uint >= self.pool.as_ptr() as uint);
        // divide rounds down so it doesn't matter if its in the middle of Slot<>
        let idx = ((ptr as uint) - (self.pool.as_ptr() as uint)) / std::mem::size_of::<Slot<T>>();
        self.freeidx(idx)
    }

    /// Return the max number of pool entries (size passed to new()).
    #[allow(dead_code)]
    pub fn limit(&self) -> uint { self.pool.len() }

    /// Return number of currently allocated entries.
    #[allow(dead_code)]
    pub fn used(&self) -> uint { self.used }

    /// Return number of remaining unused entries.
    #[allow(dead_code)]
    pub fn avail(&self) -> uint { self.limit() - self.used() }
}

impl<T> Index<uint,T> for Pool<T> {
    fn index(&self, idx: &uint) -> &T {
        match self.pool[*idx] {
            Slot::Free(_) => panic!("access free index {}", idx),
            Slot::Alloc(ref t) => t
        }
    }
}

impl<T> IndexMut<uint,T> for Pool<T> {
    fn index_mut(&mut self, idx: &uint) -> &mut T {
        match &mut self.pool[*idx] {
            &Slot::Free(_) => panic!("access free index {}", idx),
            &Slot::Alloc(ref mut t) => t
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

        for i in range(0i,4) {
            let idx = p.allocidx(i);

            assert!(p.used() == (i + 1) as uint);
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

        for i in range(0i, 20) {
            let idx = p.allocidx(i);

            assert!(idx.is_ok());
            assert!(idx.unwrap() < 4);
            assert!(p[idx.unwrap()] == i);

            v.push(idx.unwrap());

            if p.avail() == 0 {
                p.freeidx(v.remove(0).unwrap());
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

        for i in range(0i, 20) {
            let idx = p.allocidx(i);

            assert!(idx.is_ok());
            assert!(idx.ok().unwrap() < 4);
            assert!(p[idx.ok().unwrap()] == i);

            v.push(&p[idx.ok().unwrap()] as *const int);

            if p.avail() == 0 {
                unsafe { p.freeptr(v.remove(0).unwrap()) };
                assert!(p.avail() == 1);
            }
        }
    }

    #[test]
    #[should_fail]
    fn badfree1() {
        let mut p = Pool::new(4);

        let idx = p.allocidx(0i);
        assert!(idx.is_ok());

        p.freeidx(idx.ok().unwrap() + 1);
    }

    #[test]
    #[should_fail]
    fn badfree2() {
        let mut p = Pool::new(4);

        let idx = p.allocidx(0i);
        assert!(idx.is_ok());

        p.freeidx(idx.ok().unwrap() - 1);
    }

    #[test]
    #[should_fail]
    fn badidx0() {
        let mut p = Pool::new(4);

        p[0] = 1i;
    }    

    #[test]
    #[should_fail]
    fn badidx1() {
        let mut p = Pool::new(4);

        let idx = p.allocidx(0i);
        assert!(idx.is_ok());

        p[idx.ok().unwrap() + 1] = 1;
    }    

    #[test]
    #[should_fail]
    fn badidx2() {
        let mut p = Pool::new(4);

        let idx = p.allocidx(0i);
        assert!(idx.is_ok());

        p[idx.ok().unwrap() - 1] = 1;
    }    

    #[test]
    #[should_fail]
    fn badptr1() {
        let mut p = Pool::new(4);
        let foo = 1i;

        let idx = p.allocidx(0i);
        assert!(idx.is_ok());

        unsafe { p.freeptr(&foo as *const int) };
    }    

    #[test]
    #[should_fail]
    fn badptr2() {
        let mut p = Pool::new(4);

        let idx = p.allocidx(0i);
        assert!(idx.is_ok());

        unsafe {
            let ptr = ((&p[0] as *const int as uint) - 256) as *const int;
            p.freeptr(ptr)
        };
    }    
}
