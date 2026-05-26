use std::sync::atomic::{
    AtomicBool, AtomicI8, AtomicI16, AtomicI32, AtomicI64, AtomicU8, AtomicU16, AtomicU32,
    AtomicU64, Ordering,
};

#[auto_impl(&, &mut, Box, Arc, Rc)]
pub trait Counter: Default {
    type Snapshot: Default;

    /// Replace the given [`dst`] with the latest snapshot of this counter.
    fn snapshot_into(&self, dst: &mut Self::Snapshot);
}

impl<T: Counter> Counter for Option<T> {
    type Snapshot = Option<T::Snapshot>;

    #[inline(always)]
    fn snapshot_into(&self, dst: &mut Self::Snapshot) {
        match (self, dst.as_mut()) {
            (None, _) => {
                *dst = None;
            }
            (Some(c), Some(s)) => c.snapshot_into(s),
            (Some(c), None) => {
                *dst = Some(T::Snapshot::default());
                let cell = unsafe {
                    // SAFETY: we literally just set it to Some
                    dst.as_mut().unwrap_unchecked()
                };

                c.snapshot_into(cell);
            }
        }
    }
}

impl<T: Counter> Counter for Vec<T> {
    type Snapshot = Vec<T::Snapshot>;

    #[inline(always)]
    fn snapshot_into(&self, dst: &mut Self::Snapshot) {
        if dst.len() != self.len() {
            // replace with a new vec of empty cells
            *dst = Vec::with_capacity(self.len());
            for _ in self {
                dst.push(T::Snapshot::default());
            }
        }

        for (c, s) in self.iter().zip(dst.iter_mut()) {
            c.snapshot_into(s);
        }
    }
}

macro_rules! define_counter_struct {
    ($name:ident<$generic:ident> {
        $($field:ident : $field_type:ty,)*
    }) => {
        #[derive(Debug, Default, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
        pub struct $name<$generic> {
            $(pub $field: $field_type,)*
        }

        impl<C: Counter> Counter for $name<C> {
            type Snapshot = $name<C::Snapshot>;

            #[inline]
            fn snapshot_into(&self, dst: &mut Self::Snapshot) {
                $(self.$field.snapshot_into(&mut dst.$field);)*
            }
        }
    };
}

use auto_impl::auto_impl;
pub(crate) use define_counter_struct;

macro_rules! impl_counter_for_atomic {
    ($atomic:ty, $snapshot:ty) => {
        impl Counter for $atomic {
            type Snapshot = $snapshot;

            #[inline(always)]
            fn snapshot_into(&self, dst: &mut Self::Snapshot) {
                *dst = self.load(Ordering::Relaxed);
            }
        }
    };
}

impl_counter_for_atomic!(AtomicBool, bool);

impl_counter_for_atomic!(AtomicI8, i8);
impl_counter_for_atomic!(AtomicI16, i16);
impl_counter_for_atomic!(AtomicI32, i32);
impl_counter_for_atomic!(AtomicI64, i64);

impl_counter_for_atomic!(AtomicU8, u8);
impl_counter_for_atomic!(AtomicU16, u16);
impl_counter_for_atomic!(AtomicU32, u32);
impl_counter_for_atomic!(AtomicU64, u64);

#[cfg(test)]
mod tests {
    use super::*;

    define_counter_struct!(TestCounters<N> {
        foo: N,
        bar: Option<TestCountersChildren<N>>,
        spam: Vec<TestCountersChildren<N>>,
    });

    define_counter_struct!(TestCountersChildren<N> {
        field: N,
    });

    #[test]
    fn define_u64_counters_works() {
        let counter = TestCounters::<AtomicU64> {
            foo: 10.into(),
            bar: None,
            spam: vec![TestCountersChildren { field: 20.into() }],
        };

        let mut actual = TestCounters::<u64>::default();
        counter.snapshot_into(&mut actual);

        assert_eq!(
            actual,
            TestCounters::<u64> {
                foo: 10,
                bar: None,
                spam: vec![TestCountersChildren { field: 20 }]
            }
        )
    }
}
