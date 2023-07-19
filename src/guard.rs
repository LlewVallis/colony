use std::fmt;
use std::fmt::{Debug, Formatter};
use std::hash::Hash;

use crate::guard::sealed::Sealed;

pub trait Guard: Sealed {
    type Handle;

    #[doc(hidden)]
    fn new_full() -> Self;

    #[doc(hidden)]
    fn new_handle(&self, index: usize) -> Self::Handle;

    #[doc(hidden)]
    fn extract_index(handle: &Self::Handle) -> usize;

    #[doc(hidden)]
    unsafe fn fill(&mut self);

    #[doc(hidden)]
    unsafe fn empty(&mut self) -> bool;
}

pub trait CheckedGuard: Guard {
    #[doc(hidden)]
    fn check(&self, handle: &Self::Handle) -> bool;
}

#[non_exhaustive]
#[allow(missing_debug_implementations)]
pub struct NoGuard;

impl Guard for NoGuard {
    type Handle = usize;

    fn new_full() -> Self {
        Self
    }

    fn new_handle(&self, index: usize) -> Self::Handle {
        index
    }

    fn extract_index(handle: &usize) -> usize {
        *handle
    }

    unsafe fn fill(&mut self) {}

    unsafe fn empty(&mut self) -> bool {
        true
    }
}

impl Sealed for NoGuard {}

#[allow(missing_debug_implementations)]
pub struct FlagGuard {
    occupied: bool,
}

impl Guard for FlagGuard {
    type Handle = usize;

    fn new_full() -> Self {
        Self { occupied: true }
    }

    fn new_handle(&self, index: usize) -> Self::Handle {
        index
    }

    fn extract_index(handle: &usize) -> usize {
        *handle
    }

    unsafe fn fill(&mut self) {
        self.occupied = true;
    }

    unsafe fn empty(&mut self) -> bool {
        self.occupied = false;
        true
    }
}

impl CheckedGuard for FlagGuard {
    fn check(&self, _handle: &usize) -> bool {
        self.occupied
    }
}

impl Sealed for FlagGuard {}

// An even value indicates an occupied slot, we can never leak an odd value
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Generation(#[cfg(not(fuzzing))] u32, #[cfg(fuzzing)] pub u32);

impl Debug for Generation {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let value = self.0 / 2;
        f.debug_tuple("Generation").field(&value).finish()
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Handle {
    pub index: usize,
    pub generation: Generation,
}

#[allow(missing_debug_implementations)]
pub struct GenerationGuard {
    generation: Generation,
}

impl Guard for GenerationGuard {
    type Handle = Handle;

    fn new_full() -> Self {
        Self {
            generation: Generation(0),
        }
    }

    fn new_handle(&self, index: usize) -> Handle {
        debug_assert!(self.generation.0 % 2 == 0);

        Handle {
            generation: self.generation,
            index,
        }
    }

    fn extract_index(handle: &Handle) -> usize {
        handle.index
    }

    unsafe fn fill(&mut self) {
        debug_assert!(self.generation.0 % 2 == 1);
        self.generation.0 += 1;
    }

    unsafe fn empty(&mut self) -> bool {
        debug_assert!(self.generation.0 % 2 == 0);
        self.generation.0 += 1;
        self.generation.0 != u32::MAX
    }
}

impl CheckedGuard for GenerationGuard {
    fn check(&self, handle: &Handle) -> bool {
        self.generation == handle.generation
    }
}

impl Sealed for GenerationGuard {}

mod sealed {
    pub trait Sealed {}
}
