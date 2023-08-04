#![no_main]

use std::collections::HashMap;
use std::num::NonZeroU64;
use libfuzzer_sys::arbitrary::Arbitrary;
use libfuzzer_sys::{arbitrary, fuzz_target};
use colony::{Colony, Handle, Generation};

type T = u8;

#[derive(Arbitrary, Debug)]
enum Operation {
    Insert(T),
    Remove((usize, u64)),
}

fuzz_target!(|operations: Vec<Operation>| {
    let mut colony = Colony::new();
    let mut values = HashMap::new();

    for operation in operations {
        match operation {
            Operation::Insert(value) => {
                let index = colony.insert(value);
                let old = values.insert(index, value);
                assert!(old.is_none());
            },
            Operation::Remove((index, state)) => {
                if state == 0 || state % 2 != 0 {
                    continue;
                }

                let state = NonZeroU64::new(state).unwrap();
                let generation = Generation { state };

                let handle = Handle { index, generation };
                let expected = values.remove(&handle);
                let actual = colony.remove(handle);
                assert_eq!(actual, expected);
            }
        }
    }

    for (index, value) in &colony {
        assert_eq!(Some(value), values.get(&index));
    }
});
