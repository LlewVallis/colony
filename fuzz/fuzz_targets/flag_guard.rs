#![no_main]

use std::collections::HashMap;
use libfuzzer_sys::arbitrary::Arbitrary;
use libfuzzer_sys::{arbitrary, fuzz_target};
use colony::{Colony, FlagGuard};

type T = u8;

#[derive(Arbitrary, Debug)]
enum Operation {
    Insert(T),
    Remove(usize),
}

fuzz_target!(|operations: Vec<Operation>| {
    let mut colony = Colony::<_, FlagGuard>::default();
    let mut values = HashMap::new();

    for operation in operations {
        match operation {
            Operation::Insert(value) => {
                let index = colony.insert(value);
                let old = values.insert(index, value);
                assert!(old.is_none());
            },
            Operation::Remove(index) => {
                let expected = values.remove(&index);
                let actual = colony.remove(index);
                assert_eq!(actual, expected);
            }
        }
    }

    for (index, value) in &colony {
        assert_eq!(Some(value), values.get(&index));
    }
});
