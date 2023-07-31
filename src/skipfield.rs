use std::mem;
use std::ptr::NonNull;

pub type SkipfieldElement = u8;

pub type Direction = i8;

pub const LEFT: Direction = -1;
pub const RIGHT: Direction = 1;

const SENTINEL: SkipfieldElement = 255;

#[derive(Copy, Clone)]
pub struct SkipfieldPtr {
    ptr: NonNull<SkipfieldElement>,
}

impl SkipfieldPtr {
    pub fn new(ptr: NonNull<SkipfieldElement>) -> Self {
        Self { ptr }
    }

    // Preconditions:
    // * index in bounds and unskipped
    pub unsafe fn skip(&self, index: usize) -> (usize, usize) {
        let left = self.read::<LEFT>(index as isize - 1);
        let right = self.read::<RIGHT>(index as isize + 1);

        debug_assert!(left
            .checked_add(right)
            .and_then(|n| n.checked_add(1))
            .is_some());

        let size = left + right + 1;

        let start = index - left;
        let end = index + right;

        self.write::<RIGHT>(start as isize, size);
        self.write::<LEFT>(end as isize, size);

        (start, end)
    }

    // Preconditions:
    // * index is the head of a skipblock
    pub unsafe fn unskip_leftmost(&self, index: usize) {
        let old_size = self.read::<RIGHT>(index as isize);
        debug_assert!(old_size > 0);

        self.write::<RIGHT>(index as isize, 0);

        let new_size = old_size - 1;

        if new_size > 0 {
            self.write::<RIGHT>((index + 1) as isize, new_size);
            self.write::<LEFT>((index + old_size - 1) as isize, new_size);
        }
    }

    // Preconditions:
    // * index is in [-1, len + 1)
    // * if there is a skipblock over index, its head is at index
    pub unsafe fn read<const DIR: Direction>(&self, index: isize) -> usize {
        let ptr = self.ptr.as_ptr().offset(index);

        if *ptr < SENTINEL {
            *ptr as usize
        } else {
            *Self::spilled_addr::<DIR>(ptr)
        }
    }

    // Preconditions:
    // * index is in [0, len)
    // * there is sufficient space for the value if it must be spilled
    unsafe fn write<const DIR: Direction>(&self, index: isize, value: usize) {
        let ptr = self.ptr.as_ptr().offset(index);

        if value < SENTINEL as usize {
            *ptr = value as SkipfieldElement;
        } else {
            *ptr = SENTINEL;
            *Self::spilled_addr::<DIR>(ptr) = value;
        }
    }

    unsafe fn spilled_addr<const DIR: Direction>(ptr: *mut SkipfieldElement) -> *mut usize {
        assert_eq!(mem::size_of::<SkipfieldElement>(), 1);

        let ptr_addr = ptr as usize;
        let usize_size = mem::size_of::<usize>();
        let offset = (usize_size as isize * DIR as isize) - (ptr_addr % usize_size) as isize;

        ptr.offset(offset) as *mut usize
    }
}

#[cfg(test)]
mod test {
    use crate::skipfield::{SkipfieldElement, SkipfieldPtr, LEFT, RIGHT};
    use std::ptr::NonNull;

    struct Model {
        field: Vec<SkipfieldElement>,
        skipped: Vec<bool>,
    }

    impl Model {
        pub fn new(size: usize) -> Self {
            Self {
                field: vec![0; size + 2],
                skipped: vec![false; size],
            }
        }

        fn len(&self) -> usize {
            self.skipped.len()
        }

        fn skipfield(&self) -> SkipfieldPtr {
            unsafe {
                let ptr = self.field.as_ptr().add(1);
                SkipfieldPtr::new(NonNull::new_unchecked(ptr as *mut _))
            }
        }

        fn skipfield_mut(&mut self) -> SkipfieldPtr {
            unsafe {
                let ptr = self.field.as_mut_ptr().add(1);
                SkipfieldPtr::new(NonNull::new_unchecked(ptr))
            }
        }

        pub fn skip(&mut self, index: usize) {
            assert!(index < self.len());
            assert!(!self.skipped[index]);

            self.skipped[index] = true;

            unsafe {
                self.skipfield_mut().skip(index);
            }
        }

        pub fn unskip_leftmost(&mut self, index: usize) {
            assert!(index < self.len());
            assert!(self.skipped[index]);
            assert!(index == 0 || !self.skipped[index - 1]);

            self.skipped[index] = false;

            unsafe {
                self.skipfield_mut().unskip_leftmost(index);
            }
        }

        pub fn check(&self) {
            let mut index = 0;

            loop {
                let skipped = unsafe { self.skipfield().read::<RIGHT>(index as isize) };

                if skipped > 0 {
                    unsafe {
                        let from_right = self
                            .skipfield()
                            .read::<LEFT>((index + skipped - 1) as isize);
                        assert_eq!(skipped, from_right);
                    }
                }

                for _ in 0..skipped {
                    assert!(self.skipped[index]);
                    index += 1;
                }

                if index >= self.len() {
                    return;
                }

                assert!(!self.skipped[index]);
                index += 1;
            }
        }
    }

    const N: &[usize] = &[0, 1, 5, 10, 100, 1_000, 10_000, 100_000];

    #[test]
    fn full() {
        for &size in N {
            let model = Model::new(size);
            model.check();
        }
    }

    #[test]
    fn skip_one() {
        let mut model = Model::new(10);
        model.skip(5);
        model.check();
    }

    #[test]
    fn skip_all() {
        for &size in N {
            let mut model = Model::new(size);

            for i in 0..size {
                model.skip(i);
            }

            model.check();
        }
    }

    #[test]
    fn join_blocks() {
        let mut model = Model::new(5);

        model.skip(0);
        model.skip(1);
        model.skip(3);
        model.skip(4);
        model.skip(2);

        model.check();
    }

    #[test]
    fn unskip_all() {
        for &size in N {
            let mut model = Model::new(size);

            for i in 0..size {
                model.skip(i);
            }

            for i in 0..size {
                model.unskip_leftmost(i);
            }

            model.check();
        }
    }
}
