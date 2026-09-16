#![expect(unused)]

//! Utilities for working with multiple candidate values using [entropy](https://en.wikipedia.org/wiki/Entropy_(information_theory))-based probabilities.
//!
//! ## Concrete example of usage
//!
//! Consider the task of determining what kind of file format a file has.
//!
//! - We can guess that the file is gzipped if it has a `.gz` extension, which
//!   can give us 24 bits of certainty: eight for each byte in the extension.
//! - If we find that the file in fact starts with `1f 8b`, the magic number for
//!   gzip, we can assign it 16 bits of certainty plus an additional fudge
//!   factor, maybe `PATH_MAX * 8`, for it being based on the actual file
//!   contents rather than the file extension.
//! - But then, if we happen to find `55 aa` at offset 510, designating a master
//!   boot record, that's now 16 bits of certainty plus `PATH_MAX * 8` towards
//!   this file not being compressed at all!
//!
//! So now we have two reasons to guess that it's gzip, summing up to `16 + 24 +
//! PATH_MAX * 8` bits of certainty, while we only have one reason to guess that
//! it's a raw disk, summing up to `16 + PATH_MAX * 8`. The additional
//! evidence from the file extension acts as a tiebreaker.

use std::{
    cmp::Ordering,
    iter::Sum,
    ops::{Add, AddAssign},
};

/// A list of candidate values.
pub struct Candidates<T: Ord> {
    inner: Vec<Candidate<T>>,
}

impl<T: Ord> Candidates<T> {
    pub fn new(inner: Vec<Candidate<T>>) -> Self {
        Self { inner }
    }

    /// Get the likeliest candidate values stored here.
    pub fn likeliest(&self) -> Vec<&Candidate<T>> {
        // Get one maximum value
        let Some(max) = self.inner.iter().max() else {
            return vec![];
        };

        // Find all values with same certainty as the maximum value to search
        // for ties
        self.inner.iter().filter(|x| *x == max).collect()
    }

    /// Get a list of all candidates stored here.
    pub fn all_candidates(&self) -> impl Iterator<Item = &Candidate<T>> {
        self.inner.iter()
    }

    /// Add a candidate to this set.
    pub fn add(&mut self, item: Candidate<T>) {
        self.inner.push(item);
    }
}

/// A candidate value `T` combined with a list of [`Reason`]s it's listed as a
/// candidate.
///
/// The `T` must implement [`Ord`] to act as a tiebreaker in case there exist
/// two candidates with the same certainty.
pub struct Candidate<T: Ord> {
    value: T,
    reasons: Vec<Reason>,
}

impl<T: Ord> Candidate<T> {
    pub fn new(value: T, reasons: Vec<Reason>) -> Self {
        Self { value, reasons }
    }

    pub fn certainty(&self) -> Certainty {
        self.reasons.iter().map(|x| x.certainty()).sum()
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

impl<T: Ord> PartialEq for Candidate<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.certainty() == other.certainty()
    }
}

impl<T: Ord> Eq for Candidate<T> {}

impl<T: Ord> PartialOrd for Candidate<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Ord> Ord for Candidate<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare certainties, then use the value itself as a tiebreaker
        self.certainty()
            .cmp(&other.certainty())
            .then(self.value.cmp(&other.value))
    }
}

/// Reason a value was provided as a candidate, combined with a certainty value
/// it provides.
pub struct Reason {
    certainty: Certainty,
    description: String,
}

impl Reason {
    pub fn new(certainty: Certainty, description: String) -> Self {
        Self {
            certainty,
            description,
        }
    }

    /// How much certainty this reason adds to this candidate.
    pub fn certainty(&self) -> Certainty {
        self.certainty
    }

    /// A string describing what this reason is.
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// A representation of how certain something is.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum Certainty {
    /// Partial (or zero) certainty, represented in bits.
    Partial(u64),

    /// Perfect certainty (infinity bits).
    Perfect,
}

impl Certainty {
    pub fn zero() -> Self {
        Certainty::Partial(0)
    }
}

impl Default for Certainty {
    fn default() -> Self {
        Certainty::zero()
    }
}

impl Add<Self> for Certainty {
    type Output = Certainty;

    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Certainty::Partial(l), Certainty::Partial(r)) => {
                Certainty::Partial(u64::saturating_add(l, r))
            }
            (Certainty::Perfect, _) | (_, Certainty::Perfect) => Certainty::Perfect,
        }
    }
}

impl AddAssign<Self> for Certainty {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sum for Certainty {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        let mut acc = Certainty::default();
        for i in iter {
            acc += i;
        }
        acc
    }
}
