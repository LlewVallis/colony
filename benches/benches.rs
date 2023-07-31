#![recursion_limit = "1024"]
#![allow(non_snake_case)]

use iai::black_box;

use colony::Colony;

struct Random {
    state: u128,
}

impl Random {
    pub fn new() -> Self {
        Self { state: 1 }
    }

    pub fn next(&mut self) -> u64 {
        self.state *= 0xda942042e4dd58b5;
        (self.state >> 64) as u64
    }
}

#[derive(Copy, Clone)]
struct Data([usize; 4]);

impl Data {
    pub fn new(value: usize) -> Self {
        Self([value; 4])
    }
}

fn grow_then_iter(size: usize, iters: usize) {
    let mut colony = Colony::new();

    for i in 0..black_box(size) {
        colony.insert(Data::new(i));
    }

    for _ in 0..black_box(iters) {
        for (handle, &value) in &colony {
            black_box((handle, value));
        }
    }
}

fn grow(size: usize) {
    grow_then_iter(size, 0)
}

fn simulate(size: usize, steps: usize) {
    assert!(size.is_power_of_two());
    let index_mask = (size * 2) - 1;

    let mut random = Random::new();
    let mut colony = Colony::flagged();

    for _ in 0..black_box(steps) {
        let modifications = size / 10;

        for _ in 0..modifications {
            let index = random.next() as usize & index_mask;

            if let Some(value) = colony.remove(index) {
                black_box(value);
            } else {
                colony.insert(Data::new(index));
            }
        }

        for (handle, &value) in &colony {
            black_box((handle, value));
        }
    }
}

fn grow_then_iter_1x(size: usize) {
    grow_then_iter(size, 1)
}

fn grow_then_iter_10x(size: usize) {
    grow_then_iter(size, 10)
}

fn grow_then_iter_100x(size: usize) {
    grow_then_iter(size, 100)
}

fn grow_then_iter_1000x(size: usize) {
    grow_then_iter(size, 1000)
}

fn simulate_small() {
    simulate(128, 100_000);
}

fn simulate_medium() {
    simulate(16 * 1024, 500);
}

fn simulate_large() {
    simulate(1024 * 1024, 50);
}

macro_rules! cases {
    ($($rest:tt)*) => {
        cases_internal!([]; $($rest)*);
    };
}

macro_rules! cases_internal {
    ([$($functions:ident)*]; $function:ident; $($rest:tt)*) => {
        cases_internal!([$($functions)* $function]; $($rest)*);
    };
    ([$($functions:ident)*]; $function:ident($param:expr); $($rest:tt)*) => {
        paste::paste! {
            fn [<$function __ $param>]() {
                $function($param)
            }

            cases_internal!([$($functions)*]; [<$function __ $param>]; $($rest)*);
        }
    };
    ([$($functions:ident)*]; 1..1 $function:ident; $($rest:tt)*) => {
        cases_internal! {
            [$($functions)*];
            $function(1);
            $($rest)*
        }
    };
    ([$($functions:ident)*]; 1..10 $function:ident; $($rest:tt)*) => {
        cases_internal! {
            [$($functions)*];
            1..1 $function;
            $function(10);
            $($rest)*
        }
    };
    ([$($functions:ident)*]; 1..100 $function:ident; $($rest:tt)*) => {
        cases_internal! {
            [$($functions)*];
            1..10 $function;
            $function(100);
            $($rest)*
        }
    };
    ([$($functions:ident)*]; 1..1k $function:ident; $($rest:tt)*) => {
        cases_internal! {
            [$($functions)*];
            1..100 $function;
            $function(1_000);
            $($rest)*
        }
    };
    ([$($functions:ident)*]; 1..10k $function:ident; $($rest:tt)*) => {
        cases_internal! {
            [$($functions)*];
            1..1k $function;
            $function(10_000);
            $($rest)*
        }
    };
    ([$($functions:ident)*]; 1..100k $function:ident; $($rest:tt)*) => {
        cases_internal! {
            [$($functions)*];
            1..10k $function;
            $function(100_000);
            $($rest)*
        }
    };
    ([$($functions:ident)*]; 1..1m $function:ident; $($rest:tt)*) => {
        cases_internal! {
            [$($functions)*];
            1..100k $function;
            $function(1_000_000);
            $($rest)*
        }
    };
    ([$($functions:ident)*];) => {
        iai::main!($($functions),*);
    };
}

cases! {
    simulate_small;
    simulate_medium;
    simulate_large;
    1..1m grow;
    1..1m grow_then_iter_1x;
    1..1m grow_then_iter_10x;
    1..100k grow_then_iter_100x;
    1..10k grow_then_iter_1000x;
}
