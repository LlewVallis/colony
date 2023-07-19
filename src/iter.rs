use std::fmt::{Debug, Formatter};
use std::iter::FusedIterator;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::{fmt, ptr};

use crate::guard::Guard;
use crate::skipfield::{SkipfieldElement, SkipfieldPtr, RIGHT};
use crate::{Colony, GenerationGuard, Slot};

struct RawIter<T, G: Guard = GenerationGuard> {
    current_skipvalue: NonNull<SkipfieldElement>,
    current_element: NonNull<Slot<T, G>>,
    current_index: usize,
    len: usize,
}

impl<T, G: Guard> RawIter<T, G> {
    pub(super) fn new(colony: &Colony<T, G>) -> Self {
        Self {
            current_skipvalue: colony.skipfield,
            current_element: colony.elements,
            current_index: 0,
            len: colony.len,
        }
    }

    unsafe fn advance(&mut self, amount: usize) {
        let current_element = self.current_element.as_ptr().add(amount);
        self.current_element = NonNull::new_unchecked(current_element);

        let current_skipvalue = self.current_skipvalue.as_ptr().add(amount);
        self.current_skipvalue = NonNull::new_unchecked(current_skipvalue);

        self.current_index += amount;
    }
}

impl<T, G: Guard> Iterator for RawIter<T, G> {
    type Item = (G::Handle, NonNull<T>);

    fn next(&mut self) -> Option<(G::Handle, NonNull<T>)> {
        if self.len == 0 {
            return None;
        }

        unsafe {
            let skipfield = SkipfieldPtr::new(self.current_skipvalue);
            let offset = skipfield.read::<RIGHT>(0);

            self.advance(offset);

            let guard = &self.current_element.as_ref().guard;
            let handle = G::new_handle(guard, self.current_index);

            let elem = ptr::addr_of_mut!((*self.current_element.as_ptr()).inner.occupied);
            let elem = NonNull::new_unchecked(elem as *mut T);

            self.advance(1);
            self.len -= 1;

            Some((handle, elem))
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<T, G: Guard> FusedIterator for RawIter<T, G> {}

impl<T, G: Guard> ExactSizeIterator for RawIter<T, G> {}

impl<T, G: Guard> Clone for RawIter<T, G> {
    fn clone(&self) -> Self {
        Self {
            current_skipvalue: self.current_skipvalue,
            current_element: self.current_element,
            current_index: self.current_index,
            len: self.len,
        }
    }
}

pub struct Iter<'a, T, G: Guard = GenerationGuard> {
    raw: RawIter<T, G>,
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

pub struct IterMut<'a, T, G: Guard = GenerationGuard> {
    raw: RawIter<T, G>,
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
