An unordered data-structure with `O(1)` lookup, removal, iteration and `O(1)` amortized insertion.
Like a faster `HashMap` that chooses its own keys.

```
# use colony::Colony;
let mut colony = Colony::new();

// Insert
let foo_handle = colony.insert("foo");
let bar_handle = colony.insert("bar");

// Remove
assert_eq!(colony.remove(foo_handle), Some("foo"));

// Lookup
assert_eq!(colony.get(foo_handle), None);
assert_eq!(colony.get(bar_handle), Some(&"bar"));

// Iteration
for (key, &value) in colony.iter() {
    assert_eq!((key, value), (bar_handle, "bar"));
}
```

You can also think of `Colony<T>` as being like a `Vec<Option<T>>`, where instead of calling `Vec::remove`, elements are removed by setting that index to `None`.
This has the advantage of not invalidating indices to other elements, and not incurring an `O(n)` shift of other elements in the list.
The disadvantages are that it wastes the space of any deleted element, and an arbitrary number of `None`s may need traversal during iteration.
`Colony` introduces some extra structure that allows reuse of a removed element's space and constant time iteration.

This crate is partly a port of [`plf::colony`](https://plflib.org/colony.htm), which is a 
[proposed addition](https://isocpp.org/files/papers/P0447R16.html)
to the C++ standard library under the name `std::hive`.
This implementation has a few differences though:
* By default, some extra metadata is included to provide a safe API.
* We use a single relocated allocation, so:
  * Insertion is `O(1)` *amortized*, not true `O(1)`.
  * `Colony` is not pointer stable (instead, index stable).
* The skipfield implementation is a bit different (see implementation section).

# Customization

`Colony` has a second type parameter, `G`, which specifies the *guard* the colony will use.
A guard is a piece of metadata included alongside each element, and it dictates the guarantees the API can make.
There are three available guards, detailed below:

## `GenerationGuard` (the default)

[`GenerationGuard`] tags each element and handle with a generation.
This means that the same handle will never be used twice by the same colony.
This lets you keep hold of handles to removed elements, without worrying that `get(handle)` will return anything but `None`.

```
# use colony::Colony;
// `GenerationGuard` is the default
let mut colony = Colony::new();

// If we insert after removing, 
// the same memory location may be reused
let foo_handle = colony.insert("foo");
colony.remove(foo_handle);
let bar_handle = colony.insert("bar");

// Although the same index is reused ...
assert_eq!(foo_handle.index, bar_handle.index);
// ... the generations are not ...
assert_ne!(foo_handle.generation, bar_handle.generation);
// ... and so we get distinct handles
assert_ne!(foo_handle, bar_handle);

// And we get our desired behavior
assert_eq!(colony.get(foo_handle), None);
assert_eq!(colony.get(bar_handle), Some(&"bar"));
```

`GenerationGuard` also tags the colony itself with a unique ID, ensuring that handles from different colonies never alias.

```
# use colony::{Colony, Handle};
let mut colony_1 = Colony::new();
let mut colony_2 = Colony::new();

let handle_1: Handle = colony_1.insert(1);
let handle_2: Handle = colony_2.insert(2);

// Both elements are at index `0` in their respective colonies
assert_eq!(handle_1.index, handle_2.index);

// But handles from different colonies still won't alias
assert_ne!(handle_1, handle_2);
assert!(colony_1.get(handle_2).is_none());
assert!(colony_2.get(handle_1).is_none());
```

Because a unique ID is created for each colony, calls to `new` may crash after `2^44 - 1` colonies have been created.
Exhuasting this limit would require creating a million colonies every second for more than 200 days.

## `FlagGuard`

[`FlagGuard`] implements the bare minimum needed for a safe API --- it tags each element with a `bool` indicating whether an element exists there.
This means that handles to removed elements may alias any other handles created after the removal.
If and when this aliasing happens is unspecified.
When using `FlagGuard`, the handles returned by a colony will just be a `usize` index rather than a [`Handle`].

```
# use colony::{Colony, FlaggedColony};
let mut colony: FlaggedColony<_> = Colony::flagged();

let foo_index: usize = colony.insert("foo");
colony.remove(foo_index);
let bar_index: usize = colony.insert("bar");

// After removal, handles/indices may alias
assert_eq!(foo_index, bar_index);
assert_eq!(colony[foo_index], colony[bar_index]);
```

`FlagGuard` does not assign unique IDs to each colony, meaning aliasing may also occur across colonies.

```
# use colony::{Colony, Handle};
let mut colony_1 = Colony::flagged();
let mut colony_2 = Colony::flagged();

let index_1 = colony_1.insert(1);
let index_2 = colony_2.insert(2);

assert_eq!(index_1, index_2);
assert_eq!(colony_1[index_2], 1);
assert_eq!(colony_2[index_1], 2);
```

## `NoGuard`

Usable of [`NoGuard`] removes much of `Colony`'s safe API.
For example, there is no `get` or `remove`, only the unchecked variants.
If you are certain you will never attempt to access an element that doesn't exist, `NoGuard` provides you a zero overhead way to do so.

```
# use colony::{Colony, UnguardedColony};
let mut colony: UnguardedColony<_> = Colony::unguarded();

let index: usize = colony.insert("foo");

unsafe {
    assert_eq!(colony.get_unchecked(index), &"foo");
}
```

# Implementation

A `Colony` has roughly the following memory layout:

```
# use std::mem::ManuallyDrop;
# use colony::Guard;
struct Colony<T, G: Guard> {
    slots: Vec<Slot<T, G>>,
    skipfield: Vec<u8>,
    // ...
}

struct Slot<T, G: Guard> {
    guard: G,
    data: SlotData<T>,
}

union SlotData<T> {
    occupied: ManuallyDrop<T>,
    empty: (usize, usize),
}
```

Since `slots` and `skipfield` always have the same length and are resized together, they are actually managed in the same allocation, rather than in two `Vec`s.
Otherwise, this is a fairly accurate representation.
From this we can determine that a `Colony<u8>`, for example, would be very inefficient since each slot would be 24 bytes on a 64-bit system.

The `usize` pair in `SlotData` is part of the intrusive linked [freelist](https://en.wikipedia.org/wiki/Free_list) maintained by the `Colony` that enables reuse of empty slots.
The `skipfield` allows for empty slots to be efficiently skipped during iteration.
It is based on the [low-complexity jump counting pattern](https://plflib.org/matt_bentley_-_the_low_complexity_jump-counting_pattern.pdf).
These are the two bits of structure that make `Colony<T>` substantially different from `Vec<Option<T>>`.

An outline of the steps performed by each fundamental operation is given below:

## Lookup

* Bounds check the index.
* Perform the indexing operation (just offsetting a pointer).
* Check the guard for the corresponding slot.

## Insertion

* If the freelist is empty:
  * Resize the allocation if there is no more space.
  * Create a new slot at the end of the colony.
* Otherwise:
  * Update the skipfield to unskip the slot.
  * Insert the element into the first slot in the freelist (and remove said slot from the freelist).

## Removal

* Perform bounds checking and check the relevant guard.
* Update the skipfield to skip the slot.
* Insert the new slot into the freelist.
  Some adjacent skiplist entries may also need to be updated to maintain some invariants.

## Iteration (per step)

* Check if there are any elements remaining.
* Read the skipfield and skip the corresponding number of elements (often this is zero elements).
* Save the current index and a pointer to the corresponding element as the result.
* Advance forward one slot.

# Status

This implementation has been unit and fuzz tested, so it should (hopefully) work without too many issues.
It hasn't, however, been thoroughly battle-tested with real world applications, so you should be careful (especially since there's a lot of `unsafe`).
Benchmarking was used to guide the implementation, so it shouldn't be entirely naive in terms of performance, but there is probably still room for improvement.
Likewise, there is room for a more feature rich API.
