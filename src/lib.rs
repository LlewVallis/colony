#![doc = include_str!("./doc.md")]
#![warn(missing_debug_implementations)]
#![warn(missing_docs)]

use std::alloc::{alloc, dealloc, handle_alloc_error, Layout, LayoutError};
use std::fmt::{Debug, Formatter};
use std::mem::ManuallyDrop;
use std::ops::{Index, IndexMut};
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::ptr::NonNull;
use std::{fmt, mem, ptr};

pub use guard::*;
pub use iter::*;

use crate::index_opt::IndexOpt;
use crate::skipfield::{SkipfieldElement, SkipfieldPtr};

mod guard;
mod index_opt;
mod iter;
mod skipfield;

/// A `Colony` that uses `FlagGuard`, see the documentation for [`Colony`] for more information about guards.
///
/// Also see [`Colony::flagged`].
pub type FlaggedColony<T> = Colony<T, FlagGuard>;

/// A `Colony` that uses `NoGuard`, see the documentation for [`Colony`] for more information about guards.
///
/// Also see [`Colony::unguarded`].
pub type UnguardedColony<T> = Colony<T, NoGuard>;

const EMPTY_SKIPFIELD: &[SkipfieldElement] = &[0, 0];

const MAX_CAPACITY: usize = isize::MAX as usize;

#[doc = include_str!("./doc.md")]
pub struct Colony<T, G: Guard = GenerationGuard> {
    elements: NonNull<Slot<T, G>>,
    // Initialized from [-1, capacity]
    // Element at -1 and elements in [len, capacity] are zero
    // Valid for reads (but not writes) even when capacity is zero
    skipfield: NonNull<SkipfieldElement>,
    capacity: usize,
    touched: usize,
    len: usize,
    next_free: IndexOpt,
    id: G::__Id,
}

impl<T> Colony<T> {
    /// Constructs an empty colony using [`GenerationGuard`].
    ///
    /// Does not allocate.
    /// See [`Colony::flagged`] and [`Colony::unguarded`] to create colonies with different guards.
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::{Colony, GenerationGuard};
    /// let colony: Colony<i32, GenerationGuard> = Colony::new();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T> FlaggedColony<T> {
    /// Constructs an empty colony using [`FlagGuard`].
    ///
    /// Does not allocate.
    /// See [`Colony::new`] and [`Colony::unguarded`] to create colonies with different guards.
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::{Colony, FlaggedColony};
    /// let colony: FlaggedColony<i32> = Colony::flagged();
    /// ```
    pub fn flagged() -> Self {
        Self::default()
    }
}

impl<T> UnguardedColony<T> {
    /// Constructs an empty colony using [`NoGuard`].
    ///
    /// Does not allocate.
    /// See [`Colony::new`] and [`Colony::flagged`] to create colonies with different guards.
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::{Colony, UnguardedColony};
    /// let colony: UnguardedColony<i32> = Colony::unguarded();
    /// ```
    pub fn unguarded() -> Self {
        Self::default()
    }
}

impl<T, G: Guard> Default for Colony<T, G> {
    fn default() -> Self {
        let skipfield = unsafe {
            let ptr = EMPTY_SKIPFIELD.as_ptr().add(1) as *mut _;
            NonNull::new_unchecked(ptr)
        };

        Self {
            elements: NonNull::dangling(),
            skipfield,
            capacity: 0,
            touched: 0,
            len: 0,
            next_free: IndexOpt::none(),
            id: G::__sentinel_id(),
        }
    }
}

impl<T, G: Guard> Colony<T, G> {
    const MIN_NON_ZERO_CAP: usize = if mem::size_of::<T>() == 1 {
        8
    } else if mem::size_of::<T>() <= 1024 {
        4
    } else {
        1
    };

    // Preconditions:
    // * index < touched
    unsafe fn slot(&self, index: usize) -> &Slot<T, G> {
        debug_assert!(index < self.touched);
        &*self.elements.as_ptr().add(index)
    }

    // Preconditions:
    // * index < touched
    unsafe fn slot_mut(&mut self, index: usize) -> &mut Slot<T, G> {
        debug_assert!(index < self.touched);
        &mut *self.elements.as_ptr().add(index)
    }

    fn skipfield(&self) -> SkipfieldPtr {
        SkipfieldPtr::new(self.skipfield)
    }

    /// Returns the total number of elements in the colony.
    ///
    ///  # Examples
    ///
    /// ```
    /// # use colony::Colony;
    /// let mut colony = Colony::new();
    ///
    /// let handle = colony.insert("foo");
    /// assert_eq!(colony.len(), 1);
    /// colony.remove(handle);
    /// assert_eq!(colony.len(), 0);
    /// ```
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if there are no elements in the colony.
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::Colony;
    /// let mut colony = Colony::new();
    /// assert!(colony.is_empty());
    /// colony.insert("foo");
    /// assert!(!colony.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the capacity of the colony.
    ///
    /// The colony will not allocate more memory unless the [`len`](Colony::len) would overflow the capacity.
    /// This means you can be sure that [`insert`](Colony::insert) will not panic while the colony's length is lesser than its capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::Colony;
    /// let mut colony = Colony::new();
    ///
    /// // No space is initially allocated
    /// assert_eq!(colony.capacity(), 0);
    ///
    /// // After insertion space is made for one or more new elements
    /// colony.insert("foo");
    /// assert!(colony.capacity() >= 1);
    /// ```
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns a reference to a element by the handle returned by [`insert`](Colony::insert).
    ///
    /// Some care needs to be taken with respect to aliasing of handles when not using [`GenerationGuard`].
    /// See [`Colony`] for more information.
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::Colony;
    /// let mut colony = Colony::new();
    /// let handle = colony.insert("foo");
    ///
    /// assert_eq!(colony.get(handle), Some(&"foo"));
    /// colony.remove(handle);
    /// assert_eq!(colony.get(handle), None);
    /// ```
    pub fn get(&self, handle: G::Handle) -> Option<&T>
    where
        G: CheckedGuard,
    {
        let index = G::__extract_index(&handle);

        if index >= self.touched {
            return None;
        }

        unsafe {
            let slot = self.slot(index);

            if !slot.guard.__check(&handle, self.id) {
                return None;
            }

            Some(slot.occupied())
        }
    }

    /// Returns a reference to an element at an index assuming that it exists.
    ///
    /// This is mostly useful with [`UnguardedColony`], where the regular [`get`](Colony::get) method cannot be used.
    ///
    /// # Safety
    ///
    /// An element must exist at the index provided.
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::Colony;
    /// let mut colony = Colony::new();
    /// let handle = colony.insert("foo");
    ///
    /// unsafe {
    ///     let result = colony.get_unchecked(handle.index);
    ///     assert_eq!(result, &"foo");
    /// }
    /// ```
    pub unsafe fn get_unchecked(&self, index: usize) -> &T {
        self.slot(index).occupied()
    }

    /// Returns a reference to a element by the handle returned by [`insert`](Colony::insert).
    ///
    /// See [`get`](Colony::get) for more information.
    pub fn get_mut(&mut self, handle: G::Handle) -> Option<&mut T>
    where
        G: CheckedGuard,
    {
        let index = G::__extract_index(&handle);

        if index >= self.touched {
            return None;
        }

        unsafe {
            let colony_id = self.id;
            let slot = self.slot_mut(index);

            if !slot.guard.__check(&handle, colony_id) {
                return None;
            }

            Some(slot.occupied_mut())
        }
    }

    /// Returns a mutable reference to an element at an index assuming that it exists.
    ///
    /// See [`get_unchecked`](Colony::get_unchecked) for more information.
    ///
    /// # Safety
    ///
    /// An element must exist at the index provided.
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut T {
        self.slot_mut(index).occupied_mut()
    }

    /// Inserts an element into the colony at an unspecified index.
    ///
    /// Some care needs to be taken with respect to aliasing of handles when not using [`GenerationGuard`].
    /// See [`Colony`] for more information.
    ///
    /// # Panics
    ///
    /// See [`reserve`](Self::reserve).
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::Colony;
    /// let mut colony = Colony::new();
    /// let handle = colony.insert("foo");
    /// assert_eq!(colony[handle], "foo");
    /// ```
    pub fn insert(&mut self, value: T) -> G::Handle {
        unsafe {
            if let Some(free) = self.next_free.as_opt() {
                self.insert_into_free(free, value)
            } else {
                self.insert_at_end(value)
            }
        }
    }

    // Preconditions:
    // * elements[free] is unoccupied and the head of its skipblock
    // * len < touched
    unsafe fn insert_into_free(&mut self, free: usize, value: T) -> G::Handle {
        debug_assert!(self.len < self.touched);

        self.skipfield().unskip_leftmost(free);
        self.remove_skipblock_from_skiplist(free, free);

        self.len += 1;

        let colony_id = self.id;
        let slot = self.slot_mut(free);
        slot.fill(value);
        G::__new_handle(&slot.guard, free, colony_id)
    }

    // Preconditions:
    // * len == touched
    unsafe fn insert_at_end(&mut self, value: T) -> G::Handle {
        if self.len == self.capacity {
            self.reserve(1);
        }

        self.insert_at_end_unchecked(value)
    }

    // Preconditions:
    // * len == touched < capacity
    unsafe fn insert_at_end_unchecked(&mut self, value: T) -> G::Handle {
        debug_assert!(self.len == self.touched);

        let slot = Slot::new_full(value);
        let handle = G::__new_handle(&slot.guard, self.touched, self.id);

        unsafe {
            self.elements.as_ptr().add(self.touched).write(slot);
        }

        self.touched += 1;
        self.len += 1;

        handle
    }

    /// Removes the element with the given handle, if it exists.
    ///
    /// Some care needs to be taken with respect to aliasing of handle when not using [`GenerationGuard`].
    /// See [`Colony`] for more information.
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::Colony;
    /// let mut colony = Colony::new();
    /// let handle = colony.insert("foo");
    /// assert_eq!(colony.remove(handle), Some("foo"));
    /// assert_eq!(colony.remove(handle), None);
    /// ```
    pub fn remove(&mut self, handle: G::Handle) -> Option<T>
    where
        G: CheckedGuard,
    {
        let index = G::__extract_index(&handle);

        if index >= self.touched {
            return None;
        }

        unsafe {
            let colony_id = self.id;
            let slot = self.slot_mut(index);

            if !slot.guard.__check(&handle, colony_id) {
                return None;
            }

            Some(self.remove_unchecked(index))
        }
    }

    /// Removes the element with the given index, assuming it exists.
    ///
    /// This is mostly useful with [`UnguardedColony`] where [`remove`](Colony::remove) cannot be used.
    ///
    /// # Safety
    ///
    /// An element must exist with the index provided.
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::Colony;
    /// let mut colony = Colony::new();
    /// let handle = colony.insert("foo");
    ///
    /// unsafe {
    ///     let result = colony.remove_unchecked(handle.index);
    ///     assert_eq!(result, "foo");
    /// }
    /// ```
    pub unsafe fn remove_unchecked(&mut self, index: usize) -> T {
        unsafe {
            let (result, reuse) = self.slot_mut(index).empty();
            let (start, end) = self.skipfield().skip(index);

            if reuse {
                let has_left = start != index;
                let has_right = end != index;

                if !has_left && !has_right {
                    self.stitch_no_left_no_right(index);
                } else if has_left && !has_right {
                    self.stitch_only_left(index);
                } else if !has_left && has_right {
                    self.stitch_only_right(index);
                } else {
                    self.stitch_left_and_right(index, start, end);
                }
            }

            self.len -= 1;
            result
        }
    }

    unsafe fn stitch_no_left_no_right(&mut self, index: usize) {
        self.add_skipblock_to_skiplist(index, index);
    }

    unsafe fn stitch_only_left(&mut self, index: usize) {
        let next = mem::replace(
            &mut self.slot_mut(index - 1).unoccupied_mut().next,
            IndexOpt::some(index),
        );

        if let Some(next) = next.as_opt() {
            self.slot_mut(next).unoccupied_mut().prev = IndexOpt::some(index);
        }

        *self.slot_mut(index).unoccupied_mut() = Unoccupied {
            prev: IndexOpt::some(index - 1),
            next,
        };
    }

    unsafe fn stitch_only_right(&mut self, index: usize) {
        let prev = mem::replace(
            &mut self.slot_mut(index + 1).unoccupied_mut().prev,
            IndexOpt::some(index),
        );

        match prev.as_opt() {
            Some(prev) => self.slot_mut(prev).unoccupied_mut().next = IndexOpt::some(index),
            None => self.next_free = IndexOpt::some(index),
        }

        *self.slot_mut(index).unoccupied_mut() = Unoccupied {
            prev,
            next: IndexOpt::some(index + 1),
        };
    }

    unsafe fn stitch_left_and_right(&mut self, index: usize, start: usize, end: usize) {
        self.remove_skipblock_from_skiplist(start, index - 1);
        self.remove_skipblock_from_skiplist(index + 1, end);
        self.add_skipblock_to_skiplist(start, end);

        self.slot_mut(index - 1).unoccupied_mut().next = IndexOpt::some(index);
        self.slot_mut(index + 1).unoccupied_mut().prev = IndexOpt::some(index);

        *self.slot_mut(index).unoccupied_mut() = Unoccupied {
            prev: IndexOpt::some(index - 1),
            next: IndexOpt::some(index + 1),
        };
    }

    // Preconditions:
    // * start and end are part of the same skipblock
    // * start <= end
    unsafe fn remove_skipblock_from_skiplist(&mut self, start: usize, end: usize) {
        // Careful not to alias first and last
        let prev = self.slot_mut(start).unoccupied().prev;
        let next = self.slot_mut(end).unoccupied().next;

        match prev.as_opt() {
            Some(prev) => self.slot_mut(prev).unoccupied_mut().next = next,
            None => self.next_free = next,
        }

        if let Some(next) = next.as_opt() {
            self.slot_mut(next).unoccupied_mut().prev = prev;
        }
    }

    // Preconditions:
    // * start <= end
    // * indices from start through end are all unoccupied, but not in the skiplist
    unsafe fn add_skipblock_to_skiplist(&mut self, start: usize, end: usize) {
        self.slot_mut(start).unoccupied_mut().prev = IndexOpt::none();
        self.slot_mut(end).unoccupied_mut().next = self.next_free;

        if let Some(old_head) = self.next_free.as_opt() {
            self.slot_mut(old_head).unoccupied_mut().prev = IndexOpt::some(end);
        }

        self.next_free = IndexOpt::some(start);
    }

    /// Removes all elements from the colony.
    ///
    /// This is equivalent to `*self = Colony::default()`, except that the capacity remains unchanged.
    /// This is an `O(n)` operation even if `T` doesn't implement `Drop`.
    ///
    /// # Panics
    ///
    /// When using [`GenerationalGuard], this may panic if all colony IDs have been exhausted.
    /// See [`Colony`] for more information about the colony ID limit.
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::Colony;
    /// let mut colony = Colony::new();
    ///
    /// let foo = colony.insert("foo");
    /// colony.clear();
    /// let bar = colony.insert("bar");
    ///
    /// assert_eq!(colony.get(foo), None);
    /// assert_eq!(colony.get(bar), Some(&"bar"));
    /// ```
    pub fn clear(&mut self) {
        if mem::needs_drop::<(G, T)>() {
            for value in self.values_mut() {
                unsafe {
                    ptr::drop_in_place(value);
                }
            }
        }

        unsafe {
            ptr::write_bytes(self.skipfield.as_ptr(), 0, self.touched);
        }

        self.id = G::__new_id();
        self.len = 0;
        self.touched = 0;
        self.next_free = IndexOpt::none();
    }

    /// Increases the capacity of the colony to at least `self.len() + additional`.
    ///
    /// If the colony is already sufficiently large, this is a no-op.
    /// This can be used as an optimization, or as a way to make sure [`insert`](Colony::insert) won't panic.
    ///
    /// # Panics
    ///
    /// * If this method allocates, an allocation failure may panic.
    /// * If the capacity would grow over `isize::MAX`.
    /// * When using [`GenerationGuard`], this method creates a unique ID for the colony upon the first allocation.
    ///   This method may panic if all available IDs have been exhausted.
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::Colony;
    /// let mut colony = Colony::<i32>::new();
    /// colony.reserve(100);
    /// assert!(colony.capacity() >= 100);
    /// ```
    pub fn reserve(&mut self, additional: usize) {
        if additional > self.capacity - self.len {
            unsafe {
                self.do_reserve(additional);
            }
        }
    }

    // Preconditions:
    // * len + additional > capacity
    #[cold]
    unsafe fn do_reserve(&mut self, additional: usize) {
        let new_cap = self.len.checked_add(additional);
        let new_cap = new_cap.filter(|&new_cap| new_cap < MAX_CAPACITY);
        let Some(new_cap) = new_cap else {
            panic!("capacity overflow");
        };

        let new_id = if self.capacity == 0 {
            Some(G::__new_id())
        } else {
            None
        };

        let new_cap = usize::max(new_cap, self.capacity * 2);
        let new_cap = usize::max(new_cap, Self::MIN_NON_ZERO_CAP);

        self.resize(new_cap);

        if let Some(new_id) = new_id {
            self.id = new_id;
        }
    }

    // Preconditions:
    // * new_cap >= touched
    unsafe fn resize(&mut self, new_cap: usize) {
        debug_assert!(new_cap >= self.touched);
        let old_cap = self.capacity;

        let (old_layout, _) = Self::layout(old_cap).unwrap_unchecked();
        let Ok((new_layout, skipfield_offset)) = Self::layout(new_cap) else {
            panic!("could not layout");
        };

        let old_alloc = self.elements.as_ptr() as *mut u8;

        debug_assert_ne!(new_layout.size(), 0);
        let new_alloc = alloc(new_layout);

        if new_alloc.is_null() {
            handle_alloc_error(new_layout);
        }

        let new_elements = new_alloc as *mut Slot<T, G>;
        let new_skipfield = new_alloc.add(skipfield_offset) as *mut SkipfieldElement;
        self.copy_memory(new_elements, new_skipfield, new_cap);

        if old_cap > 0 {
            debug_assert_ne!(old_layout.size(), 0);
            dealloc(old_alloc, old_layout);
        }

        self.elements = NonNull::new_unchecked(new_elements);
        self.skipfield = NonNull::new_unchecked(new_skipfield);
        self.capacity = new_cap;
    }

    // Preconditions:
    // * new_elements, new_skipfield were allocated from a layout of capacity new_cap
    // * new_cap >= touched
    unsafe fn copy_memory(
        &self,
        new_elements: *mut Slot<T, G>,
        new_skipfield: *mut SkipfieldElement,
        new_cap: usize,
    ) {
        debug_assert!(new_cap >= self.touched);
        ptr::copy_nonoverlapping(self.elements.as_ptr(), new_elements, self.touched);
        self.copy_skipfield(new_skipfield, new_cap);
    }

    // Preconditions:
    // * new_skipfield was allocated from a layout of capacity new_cap
    // * new_cap >= touched
    unsafe fn copy_skipfield(&self, new_skipfield: *mut SkipfieldElement, new_cap: usize) {
        let true_old_skipfield = self.skipfield.as_ptr().sub(1);
        let true_old_skipfield_len = self.touched + 2;
        let true_new_skipfield = new_skipfield.sub(1);
        let true_new_skipfield_len = new_cap + 2;
        let remaining_skipfield = true_new_skipfield.add(true_old_skipfield_len);

        ptr::copy_nonoverlapping(
            true_old_skipfield,
            true_new_skipfield,
            true_old_skipfield_len,
        );

        ptr::write_bytes(
            remaining_skipfield,
            0,
            true_new_skipfield_len - true_old_skipfield_len,
        );
    }

    fn layout(capacity: usize) -> Result<(Layout, usize), LayoutError> {
        let layout = Layout::array::<Slot<T, G>>(capacity)?;
        let (layout, _) = layout.extend(Layout::new::<SkipfieldElement>())?;
        let (layout, skipfield_offset) =
            layout.extend(Layout::array::<SkipfieldElement>(capacity)?)?;
        let (layout, _) = layout.extend(Layout::new::<SkipfieldElement>())?;

        Ok((layout, skipfield_offset))
    }

    /// Creates an iterator over the values in the colony and their handles.
    ///
    /// If you want an iterator over only the values (and not the handles) then call [`values`](Colony::values).
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::Colony;
    /// let mut colony = Colony::new();
    /// let foo = colony.insert("foo");
    /// let bar = colony.insert("bar");
    ///
    /// let expected = [(foo, &"foo"), (bar, &"bar")].into_iter();
    /// assert!(Iterator::eq(colony.iter(), expected));
    /// ```
    pub fn iter(&self) -> Iter<T, G> {
        Iter::new(self)
    }

    /// Creates an iterator over just the values of the colony.
    ///
    /// If you want to iterate over the handle for each value too, call [`iter`](Colony::iter).
    ///
    /// # Examples
    ///
    /// ```
    /// # use colony::Colony;
    /// let mut colony = Colony::new();
    /// colony.insert("foo");
    /// colony.insert("bar");
    ///
    /// let expected = ["foo", "bar"].iter();
    /// assert!(Iterator::eq(colony.values(), expected));
    /// ```
    pub fn values(&self) -> Values<T, G> {
        Values::new(self)
    }

    /// Creates an iterator over the values in the colony and their handles, by mutable reference.
    ///
    /// See [`iter`](Colony::iter).
    pub fn iter_mut(&mut self) -> IterMut<T, G> {
        IterMut::new(self)
    }

    /// Creates an iterator over just the values in the colony, by mutable reference.
    ///
    /// See [`values`](Colony::values).
    pub fn values_mut(&mut self) -> ValuesMut<T, G> {
        ValuesMut::new(self)
    }
}

impl<T, G: Guard> Drop for Colony<T, G> {
    fn drop(&mut self) {
        unsafe {
            if mem::needs_drop::<T>() {
                for value in self.values_mut() {
                    ptr::drop_in_place(value);
                }
            }

            if self.capacity > 0 {
                let (layout, _) = Self::layout(self.capacity).unwrap_unchecked();
                dealloc(self.elements.as_ptr() as *mut u8, layout);
            }
        }
    }
}

impl<T, G: CheckedGuard> Index<G::Handle> for Colony<T, G> {
    type Output = T;

    fn index(&self, index: G::Handle) -> &T {
        self.get(index)
            .expect("no element with that handle exists in this colony")
    }
}

impl<T, G: CheckedGuard> IndexMut<G::Handle> for Colony<T, G> {
    fn index_mut(&mut self, index: G::Handle) -> &mut T {
        self.get_mut(index)
            .expect("no element with that handle exists in this colony")
    }
}

impl<T, G: Guard> Extend<T> for Colony<T, G> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        let mut iter = iter.into_iter();

        unsafe {
            while let Some(free) = self.next_free.as_opt() {
                let Some(value) = iter.next() else { return };
                self.insert_into_free(free, value);
            }

            while let Some(value) = iter.next() {
                if self.len == self.capacity {
                    let (lower, _) = iter.size_hint();
                    self.reserve(lower.saturating_add(1));
                }

                self.insert_at_end_unchecked(value);
            }
        }
    }
}

impl<T, G: Guard> FromIterator<T> for Colony<T, G> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut result = Self::default();
        result.extend(iter);
        result
    }
}

impl<T: Clone, G: Guard> Clone for Colony<T, G> {
    fn clone(&self) -> Self {
        Self::from_iter(self.values().cloned())
    }
}

impl<T: Debug, G: Guard> Debug for Colony<T, G> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let iter = self.iter().map(|(_, value)| value);
        f.debug_list().entries(iter).finish()
    }
}

impl<'a, T, G: Guard> IntoIterator for &'a Colony<T, G> {
    type Item = (G::Handle, &'a T);
    type IntoIter = Iter<'a, T, G>;

    fn into_iter(self) -> Self::IntoIter {
        Iter::new(self)
    }
}

impl<'a, T, G: Guard> IntoIterator for &'a mut Colony<T, G> {
    type Item = (G::Handle, &'a mut T);
    type IntoIter = IterMut<'a, T, G>;

    fn into_iter(self) -> Self::IntoIter {
        IterMut::new(self)
    }
}

unsafe impl<T, G: Guard> Send for Colony<T, G>
where
    T: Send,
    G: Send,
{
}

unsafe impl<T, G: Guard> Sync for Colony<T, G>
where
    T: Sync,
    G: Sync,
{
}

impl<T, G: Guard> UnwindSafe for Colony<T, G>
where
    T: UnwindSafe,
    G: UnwindSafe,
{
}

impl<T, G: Guard> RefUnwindSafe for Colony<T, G>
where
    T: RefUnwindSafe,
    G: RefUnwindSafe,
{
}

struct Slot<T, G: Guard> {
    guard: G,
    inner: SlotInner<T>,
}

union SlotInner<T> {
    occupied: ManuallyDrop<T>,
    unoccupied: Unoccupied,
}

#[derive(Copy, Clone)]
struct Unoccupied {
    prev: IndexOpt,
    next: IndexOpt,
}

impl<T, G: Guard> Slot<T, G> {
    pub unsafe fn occupied(&self) -> &T {
        &self.inner.occupied
    }

    pub unsafe fn occupied_mut(&mut self) -> &mut T {
        &mut self.inner.occupied
    }

    pub unsafe fn unoccupied(&self) -> &Unoccupied {
        &self.inner.unoccupied
    }

    pub unsafe fn unoccupied_mut(&mut self) -> &mut Unoccupied {
        &mut self.inner.unoccupied
    }

    pub unsafe fn new_full(value: T) -> Self {
        Self {
            guard: G::__new(),
            inner: SlotInner {
                occupied: ManuallyDrop::new(value),
            },
        }
    }

    pub unsafe fn fill(&mut self, value: T) {
        self.guard.__fill();

        self.inner = SlotInner {
            occupied: ManuallyDrop::new(value),
        };
    }

    pub unsafe fn empty(&mut self) -> (T, bool) {
        let value = ManuallyDrop::take(&mut self.inner.occupied);

        self.inner = SlotInner {
            unoccupied: Unoccupied {
                prev: IndexOpt::none(),
                next: IndexOpt::none(),
            },
        };

        let reuse = self.guard.__empty();
        (value, reuse)
    }
}

#[cfg(test)]
mod test {
    use std::cmp::Ordering;
    use std::fmt::{Debug, Formatter};
    use std::sync::Arc;
    use std::{fmt, iter, mem, slice};

    use crate::{Colony, Handle, UnguardedColony};

    const N: &[usize] = &[0, 1, 5, 10, 100, 1_000, 10_000, 100_000];

    #[derive(Clone)]
    struct Model<T> {
        slots: Vec<Option<T>>,
        colony: UnguardedColony<T>,
    }

    impl<T> Model<T> {
        fn new() -> Self {
            Self {
                slots: Vec::new(),
                colony: Colony::default(),
            }
        }

        pub fn insert(&mut self, value: T) -> usize
        where
            T: Clone,
        {
            let index = self.colony.insert(value.clone());

            match index.cmp(&self.slots.len()) {
                Ordering::Equal => self.slots.push(Some(value)),
                Ordering::Less => {
                    assert!(self.slots[index].is_none());
                    self.slots[index] = Some(value);
                }
                Ordering::Greater => panic!("out of bounds index"),
            }

            index
        }

        pub fn remove(&mut self, index: usize)
        where
            T: Eq + Debug,
        {
            assert!(index < self.slots.len());
            let Some(expected) = self.slots[index].take() else {
                panic!("not occupied");
            };

            let actual = unsafe { self.colony.remove_unchecked(index) };

            assert_eq!(actual, expected);
        }

        pub fn check(&self)
        where
            T: Eq,
        {
            let expected = self.slots.iter().filter_map(|slot| slot.as_ref());
            let actual = self.colony.iter().map(|(_, value)| value);
            assert!(Iterator::eq(actual, expected));
        }
    }

    impl<T: Debug> Debug for Model<T> {
        fn fmt(&self, f: &mut Formatter) -> fmt::Result {
            #[derive(Debug)]
            #[allow(unused)]
            enum Slot<'a, T> {
                Occupied(&'a T),
                Unoccupied {
                    prev: Option<usize>,
                    next: Option<usize>,
                },
            }

            let mut slots = Vec::new();

            for (i, slot) in self.slots.iter().enumerate() {
                let slot = match slot {
                    Some(value) => Slot::Occupied(value),
                    None => unsafe {
                        let super::Unoccupied { prev, next } = self.colony.slot(i).unoccupied();

                        Slot::Unoccupied {
                            prev: prev.as_opt(),
                            next: next.as_opt(),
                        }
                    },
                };

                slots.push(slot);
            }

            let skipfield = unsafe {
                slice::from_raw_parts(self.colony.skipfield.as_ptr(), self.colony.touched)
            };

            f.debug_struct("Model")
                .field("len", &self.colony.len)
                .field("touched", &self.colony.touched)
                .field("capacity", &self.colony.capacity)
                .field("next_free", &self.colony.next_free.as_opt())
                .field("slots", &slots)
                .field("skipfield", &skipfield)
                .finish()
        }
    }

    #[test]
    fn drops() {
        for &size in N {
            let arc = Arc::new(());
            let mut colony = Colony::new();

            for _ in 0..size {
                colony.insert(arc.clone());
            }

            assert_eq!(Arc::strong_count(&arc), size + 1);
            drop(colony);
            assert_eq!(Arc::strong_count(&arc), 1);
        }
    }

    #[test]
    fn different_colonies_dont_alias() {
        let mut colony_1 = Colony::new();
        let handle_1 = colony_1.insert(1);

        let mut colony_2 = Colony::new();
        let handle_2 = colony_2.insert(1);

        assert_ne!(handle_1, handle_2);
        assert!(colony_1.get(handle_2).is_none());
        assert!(colony_2.get(handle_1).is_none());
    }

    #[test]
    fn clear() {
        let mut colony = Colony::new();
        let handle = colony.insert(42);
        colony.clear();
        assert!(colony.get(handle).is_none());
    }

    #[test]
    fn insert_after_clear_doesnt_alias() {
        let mut colony = Colony::new();

        let handle_1 = colony.insert(1);
        colony.clear();
        let handle_2 = colony.insert(2);

        assert_eq!(handle_1.index, handle_2.index);
        assert_ne!(handle_1, handle_2);
        assert!(colony.get(handle_1).is_none());
    }

    #[test]
    fn handle_is_null_pointer_optimized() {
        assert_eq!(mem::size_of::<Handle>(), 16);
        assert_eq!(mem::size_of::<Option<Handle>>(), 16);
    }

    #[test]
    fn get() {
        let mut colony = Colony::new();
        let handle = colony.insert(42);
        assert_eq!(colony.get(handle), Some(&42));
    }

    #[test]
    fn get_after_remove_generation() {
        let mut colony = Colony::new();

        let handle = colony.insert(42);
        colony.remove(handle);

        assert_eq!(colony.get(handle), None);
    }

    #[test]
    fn get_after_remove_flag() {
        let mut colony = Colony::flagged();

        let handle = colony.insert(42);
        colony.remove(handle);

        assert_eq!(colony.get(handle), None);
    }

    #[test]
    fn get_after_readd() {
        let mut colony = Colony::new();

        let handle_1 = colony.insert(42);
        colony.remove(handle_1);
        let handle_2 = colony.insert(42);

        assert_ne!(handle_1, handle_2);
        assert_eq!(colony.get(handle_1), None);
    }

    #[test]
    fn reserve() {
        fn test<T>(size: usize) {
            let mut colony = Colony::<T>::new();
            colony.reserve(size);
        }

        for &size in N {
            test::<()>(size);
            test::<u8>(size);
            test::<u32>(size);
            test::<[u32; 32]>(size);
        }
    }

    #[test]
    fn insert() {
        fn test<I>(values: I)
        where
            I: Iterator,
            I::Item: Eq + Clone,
        {
            let mut model = Model::new();

            for (i, value) in values.enumerate() {
                let index = model.insert(value);
                assert_eq!(index, i);
            }

            model.check();
        }

        for &size in N {
            test(iter::repeat(()).take(size));
            test(iter::repeat(42u8).take(size));
            test(iter::repeat(42u32).take(size));
            test(iter::repeat([42u32; 32]).take(size));
        }
    }

    #[test]
    fn remove_all_forward() {
        for &size in N {
            let mut model = Model::new();

            for i in 0..size {
                model.insert(i);
            }

            for i in 0..size {
                model.remove(i);
            }

            model.check();
        }
    }

    #[test]
    fn remove_all_backward() {
        for &size in N {
            let mut model = Model::new();

            for i in 0..size {
                model.insert(i);
            }

            for i in (0..size).rev() {
                model.remove(i);
            }

            model.check();
        }
    }

    #[test]
    fn reuse_slot() {
        for &size in N {
            if size == 0 {
                continue;
            }

            let mut model = Model::new();

            for i in 0..size {
                model.insert(i);
            }

            for i in 0..size {
                model.remove(i);

                let index = model.insert(i);
                assert_eq!(index, i);
            }

            model.check();
        }
    }

    #[test]
    fn join_skipblocks() {
        let mut model = Model::new();

        for i in 0..5 {
            model.insert(i);
        }

        model.remove(1);
        model.remove(3);
        model.remove(2);

        model.check();
    }

    #[test]
    fn remove_and_readd_twice() {
        let mut model = Model::new();

        assert_eq!(model.insert(1), 0);
        model.remove(0);
        assert_eq!(model.insert(2), 0);
        assert_eq!(model.insert(3), 1);

        model.check();
    }

    #[test]
    fn insert_after_skipblock_join() {
        let mut model = Model::new();

        assert_eq!(model.insert(1), 0);
        assert_eq!(model.insert(2), 1);
        assert_eq!(model.insert(3), 2);

        model.remove(0);
        model.remove(2);
        model.remove(1);

        assert_eq!(model.insert(5), 0);

        model.check();
    }

    #[test]
    fn skipblock_join_and_reinsert_with_other_skipblock() {
        let mut model = Model::new();

        model.insert(1);
        model.insert(2);
        model.insert(3);
        model.insert(4);
        model.insert(5);

        model.remove(4);
        model.remove(0);
        model.remove(2);
        model.remove(1);

        model.insert(6);
        model.insert(7);
        model.insert(8);
        model.insert(9);
        model.insert(10);

        model.check();
    }

    #[test]
    fn multiple_skipblocks_with_join() {
        let mut model = Model::new();

        model.insert(1);
        model.insert(1);
        model.insert(1);
        model.insert(1);
        model.insert(1);
        model.insert(1);

        model.remove(2);
        model.remove(5);
        model.remove(0);
        model.remove(1);

        model.insert(1);
        model.insert(1);

        model.remove(4);

        model.insert(1);
        model.insert(1);
        model.insert(1);

        model.check();
    }
}
