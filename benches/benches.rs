use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rand::prelude::*;

use colony::Colony;

const ITERATE_SIZES: &[usize] = &[1_000, 10_000, 100_000, 1_000_000, 10_000_000];

#[derive(Copy, Clone)]
struct Data([usize; 4]);

impl From<usize> for Data {
    fn from(value: usize) -> Self {
        Self([value; 4])
    }
}

fn build_colony(size: usize, ratio: f64) -> Colony<Data> {
    let mut rng = SmallRng::seed_from_u64(0);

    let mut colony = Colony::new();
    let mut to_remove = Vec::new();

    for i in 0..size {
        colony.insert(i.into());

        if !rng.gen_bool(ratio) {
            to_remove.push(i);
        }
    }

    to_remove.shuffle(&mut rng);

    for i in to_remove {
        unsafe {
            colony.remove_unchecked(i);
        }
    }

    colony
}

fn iterate(c: &mut Criterion, ratio: f64) {
    let mut group = c.benchmark_group(format!("iterate with {:.0}% occupied", ratio * 100.0));

    for &size in ITERATE_SIZES {
        let colony = build_colony(size, ratio);
        group.throughput(Throughput::Elements(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &colony, |b, input| {
            b.iter(|| {
                for (handle, &value) in input.iter() {
                    black_box(handle);
                    black_box(value);
                }
            });
        });
    }

    group.finish();
}

pub fn iterate_dense(c: &mut Criterion) {
    iterate(c, 1.0);
}

pub fn iterate_half(c: &mut Criterion) {
    iterate(c, 0.5);
}

pub fn iterate_quater(c: &mut Criterion) {
    iterate(c, 0.25);
}

pub fn iterate_1pt(c: &mut Criterion) {
    iterate(c, 0.01);
}

criterion_group!(
    benches,
    iterate_dense,
    iterate_half,
    iterate_quater,
    iterate_1pt
);

criterion_main!(benches);
