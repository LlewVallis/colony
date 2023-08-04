use std::fmt;
use std::fmt::{Debug, Formatter};
use std::hash::Hash;
use std::num::NonZeroU64;
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
    fn __sentinel_id() -> Self::__Id;

    #[doc(hidden)]
    fn __new_id() -> Self::__Id;

    // Preconditions:
    // * colony_id was created by __new_id
    #[doc(hidden)]
    unsafe fn __new_handle(&self, index: usize, colony_id: Self::__Id) -> Self::Handle;

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

    fn __sentinel_id() {}

    fn __new_id() {}

    unsafe fn __new_handle(&self, index: usize, _colony_id: ()) -> usize {
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

    fn __sentinel_id() {}

    fn __new_id() {}

    unsafe fn __new_handle(&self, index: usize, _colony_id: ()) -> usize {
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

const SENTINEL_COLONY_ID: u64 = 0;

const GENERATION_BITS: u32 = u64::BITS - COLONY_ID_BITS;
const MAX_GENERATION: u32 = u32::pow(2, GENERATION_BITS) - 1;

/// An opaque generation assigned to a [`Handle`].
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Generation {
    // Most significant `COLONY_ID_BITS` are for the colony ID, rest are the generation
    #[cfg(not(fuzzing))]
    state: NonZeroU64,
    #[cfg(fuzzing)]
    #[allow(missing_docs)]
    pub state: NonZeroU64,
}

impl Generation {
    // Preconditions:
    // * 0 < colony_id <= MAX_COLONY_ID
    unsafe fn new(colony_id: u64, generation: u32) -> Self {
        debug_assert_ne!(colony_id, 0);
        debug_assert!(colony_id <= MAX_COLONY_ID);
        debug_assert!(generation <= MAX_GENERATION);

        let state = (colony_id << GENERATION_BITS) | (generation as u64);

        unsafe {
            let state = NonZeroU64::new_unchecked(state);
            Self { state }
        }
    }

    fn generation(&self) -> u32 {
        let mask = (1 << GENERATION_BITS) - 1;
        (self.state.get() & mask) as u32
    }

    fn colony_id(&self) -> u64 {
        self.state.get() >> GENERATION_BITS
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
///
/// With the current implementation on 64-bit systems this type uses 16 bytes of memory and can be null pointer optimized.
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

    fn __sentinel_id() -> u64 {
        SENTINEL_COLONY_ID
    }

    fn __new_id() -> u64 {
        static NEXT_COLONY_ID: AtomicU64 = AtomicU64::new(SENTINEL_COLONY_ID + 1);

        let result = NEXT_COLONY_ID.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
            Some(id + 1).filter(|&new_id| new_id <= MAX_COLONY_ID)
        });

        let result =
            result.unwrap_or_else(|_| panic!("create create more than {} colonies", MAX_COLONY_ID));

        debug_assert_ne!(result, 0);

        result
    }

    unsafe fn __new_handle(&self, index: usize, colony_id: u64) -> Handle {
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
