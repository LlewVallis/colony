use std::fmt::{Debug, Formatter};
use std::iter::FusedIterator;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::{fmt, ptr};

use crate::guard::Guard;
use crate::skipfield::{SkipfieldPtr, RIGHT};
use crate::{Colony, GenerationGuard};

struct RawIter<'a, T, G: Guard = GenerationGuard> {
    colony: &'a Colony<T, G>,
    current_index: usize,
    len: usize,
}

impl<'a, T, G: Guard> RawIter<'a, T, G> {
    pub(super) fn new(colony: &'a Colony<T, G>) -> Self {
        Self {
            colony,
            current_index: 0,
            len: colony.len,
        }
    }
}

impl<'a, T, G: Guard> Iterator for RawIter<'a, T, G> {
    type Item = (G::Handle, NonNull<T>);

    fn next(&mut self) -> Option<(G::Handle, NonNull<T>)> {
        if self.len == 0 {
            return None;
        }

        unsafe {
            let skipfield = SkipfieldPtr::new(self.colony.skipfield);
            let offset = skipfield.read::<RIGHT>(self.current_index as isize);
            self.current_index += offset;

            let slot = self.colony.elements.as_ptr().add(self.current_index);
            let guard = &(*slot).guard;
            let handle = G::__new_handle(guard, self.current_index, self.colony.id);

            let elem = ptr::addr_of_mut!((*slot).inner.occupied);
            let elem = NonNull::new_unchecked(elem as *mut T);

            self.current_index += 1;
            self.len -= 1;

            Some((handle, elem))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<'a, T, G: Guard> FusedIterator for RawIter<'a, T, G> {}

impl<'a, T, G: Guard> ExactSizeIterator for RawIter<'a, T, G> {}

impl<'a, T, G: Guard> Clone for RawIter<'a, T, G> {
    fn clone(&self) -> Self {
        Self {
            colony: self.colony,
            current_index: self.current_index,
            len: self.len,
        }
    }
}

/// The iterator returned by [`Colony::iter`].
pub struct Iter<'a, T, G: Guard = GenerationGuard> {
    raw: RawIter<'a, T, G>,
    _marker: PhantomData<&'a T>,
}

impl<'a, T, G: Guard> Iter<'a, T, G> {
    pub(super) fn new(colony: &'a Colony<T, G>) -> Self {
        Self {
            raw: RawIter::new(colony),
            _marker: PhantomData,
        }
    }
}

impl<'a, T, G: Guard> Iterator for Iter<'a, T, G> {
    type Item = (G::Handle, &'a T);

    fn next(&mut self) -> Option<(G::Handle, &'a T)> {
        let (handle, ptr) = self.raw.next()?;
        unsafe { Some((handle, ptr.as_ref())) }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.raw.size_hint()
    }
}

impl<'a, T, G: Guard> FusedIterator for Iter<'a, T, G> {}

impl<'a, T, G: Guard> ExactSizeIterator for Iter<'a, T, G> {}

impl<'a, T, G: Guard> Clone for Iter<'a, T, G> {
    fn clone(&self) -> Self {
        Self {
            raw: self.raw.clone(),
            _marker: PhantomData,
        }
    }
}

impl<'a, T: Debug, G: Guard> Debug for Iter<'a, T, G>
where
    G::Handle: Debug,
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_list().entries(self.clone()).finish()
    }
}

/// The iterator returned by [`Colony::values`].
pub struct Values<'a, T, G: Guard = GenerationGuard> {
    iter: Iter<'a, T, G>,
}

impl<'a, T, G: Guard> Values<'a, T, G> {
    pub(super) fn new(colony: &'a Colony<T, G>) -> Self {
        Self {
            iter: Iter::new(colony),
        }
    }
}

impl<'a, T, G: Guard> Iterator for Values<'a, T, G> {
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        self.iter.next().map(|(_, value)| value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a, T, G: Guard> FusedIterator for Values<'a, T, G> {}

impl<'a, T, G: Guard> ExactSizeIterator for Values<'a, T, G> {}

impl<'a, T, G: Guard> Clone for Values<'a, T, G> {
    fn clone(&self) -> Self {
        Self {
            iter: self.iter.clone(),
        }
    }
}

impl<'a, T: Debug, G: Guard> Debug for Values<'a, T, G> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_list().entries(self.clone()).finish()
    }
}

/// The iterator returned by [`Colony::iter_mut`].
pub struct IterMut<'a, T, G: Guard = GenerationGuard> {
    raw: RawIter<'a, T, G>,
    _marker: PhantomData<&'a mut T>,
}

impl<'a, T, G: Guard> IterMut<'a, T, G> {
    pub(super) fn new(colony: &'a mut Colony<T, G>) -> Self {
        Self {
            raw: RawIter::new(colony),
            _marker: PhantomData,
        }
    }

    fn reborrow(&self) -> Iter<T, G> {
        Iter {
            raw: self.raw.clone(),
            _marker: PhantomData,
        }
    }
}

impl<'a, T, G: Guard> Iterator for IterMut<'a, T, G> {
    type Item = (G::Handle, &'a mut T);

    fn next(&mut self) -> Option<(G::Handle, &'a mut T)> {
        let (handle, mut ptr) = self.raw.next()?;
        unsafe { Some((handle, ptr.as_mut())) }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.raw.size_hint()
    }
}

impl<'a, T, G: Guard> FusedIterator for IterMut<'a, T, G> {}

impl<'a, T, G: Guard> ExactSizeIterator for IterMut<'a, T, G> {}

impl<'a, T: Debug, G: Guard> Debug for IterMut<'a, T, G>
where
    G::Handle: Debug,
{
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_list().entries(self.reborrow()).finish()
    }
}

/// The iterator returned by [`Colony::values_mut`].
pub struct ValuesMut<'a, T, G: Guard = GenerationGuard> {
    iter: IterMut<'a, T, G>,
}

impl<'a, T, G: Guard> ValuesMut<'a, T, G> {
    pub(super) fn new(colony: &'a mut Colony<T, G>) -> Self {
        Self {
            iter: IterMut::new(colony),
        }
    }

    fn reborrow(&self) -> Values<T, G> {
        Values {
            iter: self.iter.reborrow(),
        }
    }
}

impl<'a, T, G: Guard> Iterator for ValuesMut<'a, T, G> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<&'a mut T> {
        self.iter.next().map(|(_, value)| value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a, T, G: Guard> FusedIterator for ValuesMut<'a, T, G> {}

impl<'a, T, G: Guard> ExactSizeIterator for ValuesMut<'a, T, G> {}

impl<'a, T: Debug, G: Guard> Debug for ValuesMut<'a, T, G> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_list().entries(self.reborrow()).finish()
    }
}
