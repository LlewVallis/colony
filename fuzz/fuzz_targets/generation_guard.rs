#![no_main]

use std::collections::HashMap;
use libfuzzer_sys::arbitrary::Arbitrary;
use libfuzzer_sys::{arbitrary, fuzz_target};
use colony::{Colony, GenerationGuard, Handle, Generation};

type T = u8;

#[derive(Arbitrary, Debug)]
enum Operation {
    Insert(T),
    Remove((usize, u32)),
}

fuzz_target!(|operations: Vec<Operation>| {
    let mut colony = Colony::<_, GenerationGuard>::default();
    let mut values = HashMap::new();

    for operation in operations {
        match operation {
            Operation::Insert(value) => {
                let index = colony.insert(value);
                let old = values.insert(index, value);
                assert!(old.is_none());
            },
            Operation::Remove((index, generation)) => {
                let generation = Generation(generation.wrapping_mul(2));

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
