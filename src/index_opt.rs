use std::hint::unreachable_unchecked;

const THRESHOLD: usize = usize::MAX;

#[derive(Copy, Clone)]
pub struct IndexOpt {
    value: usize,
}

impl IndexOpt {
    pub fn none() -> Self {
        Self { value: THRESHOLD }
    }

    // Preconditions:
    // * value <= isize::MAX
    pub unsafe fn some(value: usize) -> Self {
        debug_assert!(value < THRESHOLD);

        #[allow(clippy::absurd_extreme_comparisons)]
        if value >= THRESHOLD {
            unreachable_unchecked();
        }

        Self { value }
    }

    pub fn as_opt(&self) -> Option<usize> {
        if self.value < THRESHOLD {
            Some(self.value)
        } else {
            None
        }
    }
}
