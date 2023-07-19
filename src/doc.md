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

This crate is partly a port of [`plf::colony`](https://plflib.org/colony.htm), which is a 
[proposed addition](https://isocpp.org/files/papers/P0447R16.html)
to the C++ standard library under the name `std::hive`.
This implementation has a few differences though:
* By default, some extra metadata is included to provide a safe API.
* We use a single relocated allocation, so:
  * Insertion is `O(1)` *amortized*, not true `O(1)`.
  * `Colony` is not pointer stable (instead, index stable).
* The skipfield implementation is a bit different (see implementation section)

# Customization

`Colony` has a second type parameter, `G`, which specifies the *guard* the colony will use.
A guard is a piece of metadata included alongside each element, and it dictates the guarantees the API can make.
There are three available guards, detailed below:

## `GenerationGuard` (the default)

[`GenerationGuard`] tags each element and handle with a `u32` generation.
This means that the same handle will never be used twice by the same colony.
This lets you keep hold of handles to removed elements, without worrying that `get(handle)` will return anything but `None`.

```
# use colony::Colony;
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

An important note though, is that two handles can still alias if they were constructed from different colonies.
In general the handles produced by distinct colonies are free to alias, and if and when they alias is unspecified.

```
# use colony::{Colony, Handle};
let mut colony_1 = Colony::new();
let mut colony_2 = Colony::new();

let handle_1: Handle = colony_1.insert(1);
let handle_2: Handle = colony_2.insert(2);

// Handles from different colonies may alias
assert_eq!(handle_1, handle_2);
assert_eq!(colony_1[handle_2], 1);
assert_eq!(colony_2[handle_1], 2);
```

## `FlagGuard`

[`FlagGuard`] implements the bare minimum needed for a safe API --- it tags each element with a `bool` indicating whether an element exists there.
This means that handles to removed elements may alias any other handles created after the removal.
Again, if and when this alias happens is unspecified.
When using `FlagGuard`, the handles returned by a colony will just be a `usize` index rather than a [`Handle`].

```
# use colony::{Colony, FlagGuard};
let mut colony = Colony::<_, FlagGuard>::default();

let foo_index: usize = colony.insert("foo");
colony.remove(foo_index);
let bar_index: usize = colony.insert("bar");

// After removal, handles/indices may alias
assert_eq!(foo_index, bar_index);
assert_eq!(colony[foo_index], colony[bar_index]);
```

## `NoGuard`

Usable of [`NoGuard`] removes much of `Colony`'s safe API.
For example, there is no `get` or `remove`, only the unchecked variants.
If you are certain you will never attempt to access an element that doesn't exist, `NoGuard` provides you a zero overhead way to do so.

```
# use colony::{Colony, NoGuard};
let mut colony = Colony::<_, NoGuard>::default();

let index: usize = colony.insert("foo");

unsafe {
    assert_eq!(colony.get_unchecked(index), &"foo");
}
```
