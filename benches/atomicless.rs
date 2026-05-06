use std::{
    cell::UnsafeCell,
    hint::black_box,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use criterion::{Criterion, criterion_group, criterion_main};

const COUNT: u64 = 1_000;

/// I thought that maybe doing this would be faster than an [`AtomicU64`], but it
/// seems that it's definitely not faster with [`Ordering::Relaxed`]; in fact, by
/// definition, both of these are doing the same exact thing.
/// That's fine though, we're keeping it here as an example of a benchmark we can do.
pub struct AtomiclessU64 {
    x: UnsafeCell<u64>,
}

unsafe impl Send for AtomiclessU64 {}
unsafe impl Sync for AtomiclessU64 {}

impl AtomiclessU64 {
    /// Load the [`u64`] stored here.
    pub fn store(&self, x: u64) {
        let ptr = self.x.get();
        unsafe {
            *ptr = x;
        }
    }

    /// Load the [`u64`] stored here.
    pub fn load(&self) -> u64 {
        let ptr = self.x.get();
        unsafe { *ptr }
    }
}

impl From<u64> for AtomiclessU64 {
    fn from(value: u64) -> Self {
        AtomiclessU64 {
            x: UnsafeCell::new(value),
        }
    }
}

impl From<AtomiclessU64> for u64 {
    fn from(value: AtomiclessU64) -> Self {
        value.x.into_inner()
    }
}

pub fn atomic(c: &mut Criterion) {
    c.benchmark_group("atomic")
        .bench_function(&format!("write{COUNT}"), |b| {
            let w = Arc::new(AtomicU64::from(0));

            b.iter(|| {
                for i in 0..COUNT {
                    black_box(w.store(i, Ordering::Relaxed));
                }
            });
        });
}

pub fn atomicless(c: &mut Criterion) {
    c.benchmark_group("atomicless")
        .bench_function(&format!("write{COUNT}"), |b| {
            let w = Arc::new(AtomiclessU64::from(0));

            b.iter(|| {
                for i in 0..COUNT {
                    black_box(w.store(i));
                }
            });
        });
}

criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = atomic, atomicless
}
criterion_main!(benches);
