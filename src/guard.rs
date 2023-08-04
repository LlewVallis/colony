use std::fmt;
use std::fmt::{Debug, Formatter};
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::guard::sealed::Sealed;

#[cfg(doc)]
use crate::Colony;

/// A guard for each element in a colony to ensure safe usage.
///
/// This is a sealed trait, so only one of the supported guards can be used.
/// Also, any `#[doc(hidden)]` member of this trait should not be considered as part of the public API.
///
/// See [`Colony`] for more information about guards.
pub trait Guard: Sealed {
    /// The type used to identify elements in a colony using this guard.
    type Handle;

    #[doc(hidden)]
    type __Id: Copy;

    #[doc(hidden)]
    fn __new() -> Self;

    #[doc(hidden)]
    fn __new_id() -> Self::__Id;

    #[doc(hidden)]
    fn __new_handle(&self, index: usize, colony_id: Self::__Id) -> Self::Handle;

    #[doc(hidden)]
    fn __extract_index(handle: &Self::Handle) -> usize;

    #[doc(hidden)]
    unsafe fn __fill(&mut self);

    #[doc(hidden)]
    unsafe fn __empty(&mut self) -> bool;
}

/// A marker trait for a [`Guard`] that enables use of safe methods like [`Colony::get`].
pub trait CheckedGuard: Guard {
    #[doc(hidden)]
    fn __check(&self, handle: &Self::Handle, colony_id: Self::__Id) -> bool;
}

/// A ZST guard that provides minimal guarantees.
///
/// See [`Colony`] for more information about guards.
#[non_exhaustive]
#[allow(missing_debug_implementations)]
pub struct NoGuard;

impl Guard for NoGuard {
    type Handle = usize;
    type __Id = ();

    fn __new() -> Self {
        Self
    }

    fn __new_id() -> Self::__Id {}

    fn __new_handle(&self, index: usize, _colony_id: ()) -> usize {
        index
    }

    fn __extract_index(handle: &usize) -> usize {
        *handle
    }

    unsafe fn __fill(&mut self) {}

    unsafe fn __empty(&mut self) -> bool {
        true
    }
}

impl Sealed for NoGuard {}

/// A `bool` guard that provides just basic safety guarantees.
///
/// See [`Colony`] for more information about guards.
#[allow(missing_debug_implementations)]
pub struct FlagGuard {
    occupied: bool,
}

impl Guard for FlagGuard {
    type Handle = usize;
    type __Id = ();

    fn __new() -> Self {
        Self { occupied: true }
    }

    fn __new_id() -> Self::__Id {}

    fn __new_handle(&self, index: usize, _colony_id: ()) -> usize {
        index
    }

    fn __extract_index(handle: &usize) -> usize {
        *handle
    }

    unsafe fn __fill(&mut self) {
        self.occupied = true;
    }

    unsafe fn __empty(&mut self) -> bool {
        self.occupied = false;
        true
    }
}

impl CheckedGuard for FlagGuard {
    fn __check(&self, _handle: &usize, _colony_id: ()) -> bool {
        self.occupied
    }
}

impl Sealed for FlagGuard {}

const COLONY_ID_BITS: u32 = 44;
const MAX_COLONY_ID: u64 = u64::pow(2, COLONY_ID_BITS) - 1;

const GENERATION_BITS: u32 = u64::BITS - COLONY_ID_BITS;
const MAX_GENERATION: u32 = u32::pow(2, GENERATION_BITS) - 1;

/// An opaque generation assigned to a [`Handle`].
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Generation {
    // Most significant `COLONY_ID_BITS` are for the colony ID, rest are the generation
    #[cfg(not(fuzzing))]
    state: u64,
    #[cfg(fuzzing)]
    #[allow(missing_docs)]
    pub state: u64,
}

impl Generation {
    fn new(colony_id: u64, generation: u32) -> Self {
        debug_assert!(colony_id <= MAX_COLONY_ID);
        debug_assert!(generation <= MAX_GENERATION);

        Self {
            state: (colony_id << GENERATION_BITS) | (generation as u64),
        }
    }

    fn generation(&self) -> u32 {
        let mask = (1 << GENERATION_BITS) - 1;
        (self.state & mask) as u32
    }

    fn colony_id(&self) -> u64 {
        self.state >> GENERATION_BITS
    }
}

impl Debug for Generation {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_tuple("Generation").field(&self.state).finish()
    }
}

/// Used to identify elements within a [`Colony`] when [`GenerationGuard`] (the default) is being used.
///
/// Pass this handle to methods such as [`Colony::get`] or [`Colony::remove`].
///
/// A handle is composed of an index and a generation.
/// When an element is removed at an index in a colony, the generation is incremented.
/// This generation is checked to make sure a handle created for a deleted element cannot be used to access a new element sharing the same index.
/// The generation also contains information about the colony itself, to prevent aliasing of handles between colonies.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Handle {
    /// The index of the element referred to by the handle.
    ///
    /// This can be used in conjunction with [`Colony::get_unchecked`], for example.
    pub index: usize,
    /// The generation of the handle.
    pub generation: Generation,
}

/// The default guard that guarantees globally unique handles.
///
/// See [`Colony`] for more information about guards.
#[allow(missing_debug_implementations)]
pub struct GenerationGuard {
    generation: u32,
}

impl Guard for GenerationGuard {
    type Handle = Handle;
    type __Id = u64;

    fn __new() -> Self {
        Self { generation: 0 }
    }

    fn __new_id() -> Self::__Id {
        static NEXT_COLONY_ID: AtomicU64 = AtomicU64::new(0);

        NEXT_COLONY_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
                Some(id + 1).filter(|&new_id| new_id <= MAX_COLONY_ID)
            })
            .unwrap_or_else(|_| panic!("create create more than {} colonies", MAX_COLONY_ID))
    }

    fn __new_handle(&self, index: usize, colony_id: u64) -> Handle {
        debug_assert!(self.generation % 2 == 0);
        let generation = Generation::new(colony_id, self.generation);
        Handle { generation, index }
    }

    fn __extract_index(handle: &Handle) -> usize {
        handle.index
    }

    unsafe fn __fill(&mut self) {
        debug_assert!(self.generation % 2 == 1);
        self.generation += 1;
    }

    unsafe fn __empty(&mut self) -> bool {
        debug_assert!(self.generation % 2 == 0);
        self.generation += 1;
        self.generation != MAX_GENERATION
    }
}

impl CheckedGuard for GenerationGuard {
    fn __check(&self, handle: &Handle, colony_id: u64) -> bool {
        colony_id == handle.generation.colony_id()
            && self.generation == handle.generation.generation()
    }
}

impl Sealed for GenerationGuard {}

mod sealed {
    pub trait Sealed {}
}
