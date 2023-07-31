#![no_main]

use std::collections::HashMap;
use libfuzzer_sys::arbitrary::Arbitrary;
use libfuzzer_sys::{arbitrary, fuzz_target};
use colony::Colony;

type T = u8;

#[derive(Arbitrary, Debug)]
enum Operation {
    Insert(T),
    Remove(usize),
}

fuzz_target!(|operations: Vec<Operation>| {
    let mut colony = Colony::unguarded();
    let mut values = HashMap::new();

    for operation in operations {
        match operation {
            Operation::Insert(value) => {
                let index = colony.insert(value);
                let old = values.insert(index, value);
                assert!(old.is_none());
            },
            Operation::Remove(index) => {
                if let Some(expected) = values.remove(&index) {
                    unsafe {
                        let actual = colony.remove_unchecked(index);
                        assert_eq!(actual, expected);
                    }
                }
            }
        }
    }

    for (index, value) in &colony {
        assert_eq!(Some(value), values.get(&index));
    }
});
