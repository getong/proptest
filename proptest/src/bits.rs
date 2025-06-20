//-
// Copyright 2017, 2018 The proptest developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Strategies for working with bit sets.
//!
//! Besides `BitSet` itself, this also defines strategies for all the primitive
//! integer types. These strategies are appropriate for integers which are used
//! as bit flags, etc; e.g., where the most reasonable simplification of `64`
//! is `0` (clearing one bit) and not `63` (clearing one bit but setting 6
//! others). For integers treated as numeric values, see the corresponding
//! modules of the `num` module instead.

use crate::std_facade::{fmt, Vec};
use core::marker::PhantomData;
use core::mem;

#[cfg(feature = "bit-set")]
use bit_set::BitSet;
#[cfg(feature = "bit-set")]
use bit_vec::BitVec;
use rand::{self, seq::IteratorRandom, Rng};

use crate::collection::SizeRange;
use crate::num::sample_uniform_incl;
use crate::strategy::*;
use crate::test_runner::*;

/// Trait for types which can be handled with `BitSetStrategy`.
#[cfg_attr(clippy, allow(len_without_is_empty))]
pub trait BitSetLike: Clone + fmt::Debug {
    /// Create a new value of `Self` with space for up to `max` bits, all
    /// initialised to zero.
    fn new_bitset(max: usize) -> Self;
    /// Return an upper bound on the greatest bit set _plus one_.
    fn len(&self) -> usize;
    /// Test whether the given bit is set.
    fn test(&self, ix: usize) -> bool;
    /// Set the given bit.
    fn set(&mut self, ix: usize);
    /// Clear the given bit.
    fn clear(&mut self, ix: usize);
    /// Return the number of bits set.
    ///
    /// This has a default for backwards compatibility, which simply does a
    /// linear scan through the bits. Implementations are strongly encouraged
    /// to override this.
    fn count(&self) -> usize {
        let mut n = 0;
        for i in 0..self.len() {
            if self.test(i) {
                n += 1;
            }
        }
        n
    }
}

macro_rules! int_bitset {
    ($typ:ty) => {
        impl BitSetLike for $typ {
            fn new_bitset(_: usize) -> Self {
                0
            }
            fn len(&self) -> usize {
                mem::size_of::<$typ>() * 8
            }
            fn test(&self, ix: usize) -> bool {
                0 != (*self & ((1 as $typ) << ix))
            }
            fn set(&mut self, ix: usize) {
                *self |= (1 as $typ) << ix;
            }
            fn clear(&mut self, ix: usize) {
                *self &= !((1 as $typ) << ix);
            }
            fn count(&self) -> usize {
                self.count_ones() as usize
            }
        }
    };
}
int_bitset!(u8);
int_bitset!(u16);
int_bitset!(u32);
int_bitset!(u64);
int_bitset!(usize);
int_bitset!(i8);
int_bitset!(i16);
int_bitset!(i32);
int_bitset!(i64);
int_bitset!(isize);

#[cfg(feature = "bit-set")]
#[cfg_attr(docsrs, doc(cfg(feature = "bit-set")))]
impl BitSetLike for BitSet {
    fn new_bitset(max: usize) -> Self {
        BitSet::with_capacity(max)
    }

    fn len(&self) -> usize {
        self.capacity()
    }

    fn test(&self, bit: usize) -> bool {
        self.contains(bit)
    }

    fn set(&mut self, bit: usize) {
        self.insert(bit);
    }

    fn clear(&mut self, bit: usize) {
        self.remove(bit);
    }

    fn count(&self) -> usize {
        self.len()
    }
}

impl BitSetLike for Vec<bool> {
    fn new_bitset(max: usize) -> Self {
        vec![false; max]
    }

    fn len(&self) -> usize {
        self.len()
    }

    fn test(&self, bit: usize) -> bool {
        if bit >= self.len() {
            false
        } else {
            self[bit]
        }
    }

    fn set(&mut self, bit: usize) {
        if bit >= self.len() {
            self.resize(bit + 1, false);
        }

        self[bit] = true;
    }

    fn clear(&mut self, bit: usize) {
        if bit < self.len() {
            self[bit] = false;
        }
    }

    fn count(&self) -> usize {
        self.iter().filter(|&&b| b).count()
    }
}

/// Generates values as a set of bits between the two bounds.
///
/// Values are generated by uniformly setting individual bits to 0
/// or 1 between the bounds. Shrinking iteratively clears bits.
#[must_use = "strategies do nothing unless used"]
#[derive(Clone, Copy, Debug)]
pub struct BitSetStrategy<T: BitSetLike> {
    min: usize,
    max: usize,
    mask: Option<T>,
}

impl<T: BitSetLike> BitSetStrategy<T> {
    /// Create a strategy which generates values where bits between `min`
    /// (inclusive) and `max` (exclusive) may be set.
    ///
    /// Due to the generics, the functions in the typed submodules are usually
    /// preferable to calling this directly.
    pub fn new(min: usize, max: usize) -> Self {
        BitSetStrategy {
            min,
            max,
            mask: None,
        }
    }

    /// Create a strategy which generates values where any bits set (and only
    /// those bits) in `mask` may be set.
    pub fn masked(mask: T) -> Self {
        BitSetStrategy {
            min: 0,
            max: mask.len(),
            mask: Some(mask),
        }
    }
}

impl<T: BitSetLike> Strategy for BitSetStrategy<T> {
    type Tree = BitSetValueTree<T>;
    type Value = T;

    fn new_tree(&self, runner: &mut TestRunner) -> NewTree<Self> {
        let mut inner = T::new_bitset(self.max);
        for bit in self.min..self.max {
            if self.mask.as_ref().map_or(true, |mask| mask.test(bit))
                && runner.rng().random()
            {
                inner.set(bit);
            }
        }

        Ok(BitSetValueTree {
            inner,
            shrink: self.min,
            prev_shrink: None,
            min_count: 0,
        })
    }
}

/// Generates bit sets with a particular number of bits set.
///
/// Specifically, this strategy is given both a size range and a bit range. To
/// produce a new value, it selects a size, then uniformly selects that many
/// bits from within the bit range.
///
/// Shrinking happens as with [`BitSetStrategy`](struct.BitSetStrategy.html).
#[derive(Clone, Debug)]
#[must_use = "strategies do nothing unless used"]
pub struct SampledBitSetStrategy<T: BitSetLike> {
    size: SizeRange,
    bits: SizeRange,
    _marker: PhantomData<T>,
}

impl<T: BitSetLike> SampledBitSetStrategy<T> {
    /// Create a strategy which generates values where bits within the bounds
    /// given by `bits` may be set. The number of bits that are set is chosen
    /// to be in the range given by `size`.
    ///
    /// Due to the generics, the functions in the typed submodules are usually
    /// preferable to calling this directly.
    ///
    /// ## Panics
    ///
    /// Panics if `size` includes a value that is greater than the number of
    /// bits in `bits`.
    pub fn new(size: impl Into<SizeRange>, bits: impl Into<SizeRange>) -> Self {
        let size = size.into();
        let bits = bits.into();
        size.assert_nonempty();

        let available_bits = bits.end_excl() - bits.start();
        assert!(
            size.end_excl() <= available_bits + 1,
            "Illegal SampledBitSetStrategy: have {} bits available, \
             but requested size is {}..{}",
            available_bits,
            size.start(),
            size.end_excl()
        );
        SampledBitSetStrategy {
            size,
            bits,
            _marker: PhantomData,
        }
    }
}

impl<T: BitSetLike> Strategy for SampledBitSetStrategy<T> {
    type Tree = BitSetValueTree<T>;
    type Value = T;

    fn new_tree(&self, runner: &mut TestRunner) -> NewTree<Self> {
        let mut bits = T::new_bitset(self.bits.end_excl());
        let count = sample_uniform_incl(
            runner,
            self.size.start(),
            self.size.end_incl(),
        );
        if bits.len() < count {
            panic!("not enough bits to sample");
        }

        for bit in self.bits.iter().choose_multiple(runner.rng(), count) {
            bits.set(bit);
        }

        Ok(BitSetValueTree {
            inner: bits,
            shrink: self.bits.start(),
            prev_shrink: None,
            min_count: self.size.start(),
        })
    }
}

/// Value tree produced by `BitSetStrategy` and `SampledBitSetStrategy`.
#[derive(Clone, Copy, Debug)]
pub struct BitSetValueTree<T: BitSetLike> {
    inner: T,
    shrink: usize,
    prev_shrink: Option<usize>,
    min_count: usize,
}

impl<T: BitSetLike> ValueTree for BitSetValueTree<T> {
    type Value = T;

    fn current(&self) -> T {
        self.inner.clone()
    }

    fn simplify(&mut self) -> bool {
        if self.inner.count() <= self.min_count {
            return false;
        }

        while self.shrink < self.inner.len() && !self.inner.test(self.shrink) {
            self.shrink += 1;
        }

        if self.shrink >= self.inner.len() {
            self.prev_shrink = None;
            false
        } else {
            self.prev_shrink = Some(self.shrink);
            self.inner.clear(self.shrink);
            self.shrink += 1;
            true
        }
    }

    fn complicate(&mut self) -> bool {
        if let Some(bit) = self.prev_shrink.take() {
            self.inner.set(bit);
            true
        } else {
            false
        }
    }
}

macro_rules! int_api {
    ($typ:ident, $max:expr) => {
        #[allow(missing_docs)]
        pub mod $typ {
            use super::*;

            /// Generates integers where all bits may be set.
            pub const ANY: BitSetStrategy<$typ> = BitSetStrategy {
                min: 0,
                max: $max,
                mask: None,
            };

            /// Generates values where bits between the given bounds may be
            /// set.
            pub fn between(min: usize, max: usize) -> BitSetStrategy<$typ> {
                BitSetStrategy::new(min, max)
            }

            /// Generates values where any bits set in `mask` (and no others)
            /// may be set.
            pub fn masked(mask: $typ) -> BitSetStrategy<$typ> {
                BitSetStrategy::masked(mask)
            }

            /// Create a strategy which generates values where bits within the
            /// bounds given by `bits` may be set. The number of bits that are
            /// set is chosen to be in the range given by `size`.
            ///
            /// ## Panics
            ///
            /// Panics if `size` includes a value that is greater than the
            /// number of bits in `bits`.
            pub fn sampled(
                size: impl Into<SizeRange>,
                bits: impl Into<SizeRange>,
            ) -> SampledBitSetStrategy<$typ> {
                SampledBitSetStrategy::new(size, bits)
            }
        }
    };
}

int_api!(u8, 8);
int_api!(u16, 16);
int_api!(u32, 32);
int_api!(u64, 64);
int_api!(i8, 8);
int_api!(i16, 16);
int_api!(i32, 32);
int_api!(i64, 64);

macro_rules! minimal_api {
    ($md:ident, $typ:ty) => {
        #[allow(missing_docs)]
        pub mod $md {
            use super::*;

            /// Generates values where bits between the given bounds may be
            /// set.
            pub fn between(min: usize, max: usize) -> BitSetStrategy<$typ> {
                BitSetStrategy::new(min, max)
            }

            /// Generates values where any bits set in `mask` (and no others)
            /// may be set.
            pub fn masked(mask: $typ) -> BitSetStrategy<$typ> {
                BitSetStrategy::masked(mask)
            }

            /// Create a strategy which generates values where bits within the
            /// bounds given by `bits` may be set. The number of bits that are
            /// set is chosen to be in the range given by `size`.
            ///
            /// ## Panics
            ///
            /// Panics if `size` includes a value that is greater than the
            /// number of bits in `bits`.
            pub fn sampled(
                size: impl Into<SizeRange>,
                bits: impl Into<SizeRange>,
            ) -> SampledBitSetStrategy<$typ> {
                SampledBitSetStrategy::new(size, bits)
            }
        }
    };
}
minimal_api!(usize, usize);
minimal_api!(isize, isize);
#[cfg(feature = "bit-set")]
#[cfg_attr(docsrs, doc(cfg(feature = "bit-set")))]
minimal_api!(bitset, BitSet);
minimal_api!(bool_vec, Vec<bool>);

pub(crate) mod varsize {
    use super::*;
    use core::iter::FromIterator;

    #[cfg(feature = "bit-set")]
    type Inner = BitSet;
    #[cfg(not(feature = "bit-set"))]
    type Inner = Vec<bool>;

    /// A bit set is a set of bit flags.
    #[derive(Debug, Clone)]
    pub struct VarBitSet(Inner);

    impl VarBitSet {
        /// Create a bit set of `len` set values.
        #[cfg(not(feature = "bit-set"))]
        pub fn saturated(len: usize) -> Self {
            Self(vec![true; len])
        }

        /// Create a bit set of `len` set values.
        #[cfg(feature = "bit-set")]
        pub fn saturated(len: usize) -> Self {
            Self(BitSet::from_bit_vec(BitVec::from_elem(len, true)))
        }

        #[cfg(not(feature = "bit-set"))]
        pub(crate) fn iter<'a>(&'a self) -> impl Iterator<Item = usize> + 'a {
            (0..self.len()).into_iter().filter(move |&ix| self.test(ix))
        }

        #[cfg(feature = "bit-set")]
        pub(crate) fn iter<'a>(&'a self) -> impl Iterator<Item = usize> + 'a {
            self.0.iter()
        }
    }

    impl BitSetLike for VarBitSet {
        fn new_bitset(max: usize) -> Self {
            VarBitSet(Inner::new_bitset(max))
        }

        fn len(&self) -> usize {
            BitSetLike::len(&self.0)
        }

        fn test(&self, bit: usize) -> bool {
            BitSetLike::test(&self.0, bit)
        }

        fn set(&mut self, bit: usize) {
            BitSetLike::set(&mut self.0, bit);
        }

        fn clear(&mut self, bit: usize) {
            BitSetLike::clear(&mut self.0, bit);
        }

        fn count(&self) -> usize {
            BitSetLike::count(&self.0)
        }
    }

    impl FromIterator<usize> for VarBitSet {
        fn from_iter<T: IntoIterator<Item = usize>>(into_iter: T) -> Self {
            let iter = into_iter.into_iter();
            let lower_bound = iter.size_hint().0;
            let mut bits = VarBitSet::new_bitset(lower_bound);
            for bit in iter {
                bits.set(bit);
            }
            bits
        }
    }

    /*
    pub(crate) fn between(min: usize, max: usize) -> BitSetStrategy<VarBitSet> {
        BitSetStrategy::new(min, max)
    }

    pub(crate) fn masked(mask: VarBitSet) -> BitSetStrategy<VarBitSet> {
        BitSetStrategy::masked(mask)
    }
    */

    pub(crate) fn sampled(
        size: impl Into<SizeRange>,
        bits: impl Into<SizeRange>,
    ) -> SampledBitSetStrategy<VarBitSet> {
        SampledBitSetStrategy::new(size, bits)
    }
}

pub use self::varsize::VarBitSet;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn generates_values_in_range() {
        let input = u32::between(4, 8);

        let mut runner = TestRunner::default();
        for _ in 0..256 {
            let value = input.new_tree(&mut runner).unwrap().current();
            assert!(0 == value & !0xF0u32, "Generate value {}", value);
        }
    }

    #[test]
    fn generates_values_in_mask() {
        let mut accum = 0;

        let mut runner = TestRunner::deterministic();
        let input = u32::masked(0xdeadbeef);
        for _ in 0..1024 {
            accum |= input.new_tree(&mut runner).unwrap().current();
        }

        assert_eq!(0xdeadbeef, accum);
    }

    #[cfg(feature = "bit-set")]
    #[test]
    fn mask_bounds_for_bitset_correct() {
        let mut seen_0 = false;
        let mut seen_2 = false;

        let mut mask = BitSet::new();
        mask.insert(0);
        mask.insert(2);

        let mut runner = TestRunner::deterministic();
        let input = bitset::masked(mask);
        for _ in 0..32 {
            let v = input.new_tree(&mut runner).unwrap().current();
            seen_0 |= v.contains(0);
            seen_2 |= v.contains(2);
        }

        assert!(seen_0);
        assert!(seen_2);
    }

    #[test]
    fn mask_bounds_for_vecbool_correct() {
        let mut seen_0 = false;
        let mut seen_2 = false;

        let mask = vec![true, false, true, false];

        let mut runner = TestRunner::deterministic();
        let input = bool_vec::masked(mask);
        for _ in 0..32 {
            let v = input.new_tree(&mut runner).unwrap().current();
            assert_eq!(4, v.len());
            seen_0 |= v[0];
            seen_2 |= v[2];
        }

        assert!(seen_0);
        assert!(seen_2);
    }

    #[test]
    fn shrinks_to_zero() {
        let input = u32::between(4, 24);

        let mut runner = TestRunner::default();
        for _ in 0..256 {
            let mut value = input.new_tree(&mut runner).unwrap();
            let mut prev = value.current();
            while value.simplify() {
                let v = value.current();
                assert!(
                    1 == (prev & !v).count_ones(),
                    "Shrank from {} to {}",
                    prev,
                    v
                );
                prev = v;
            }

            assert_eq!(0, value.current());
        }
    }

    #[test]
    fn complicates_to_previous() {
        let input = u32::between(4, 24);

        let mut runner = TestRunner::default();
        for _ in 0..256 {
            let mut value = input.new_tree(&mut runner).unwrap();
            let orig = value.current();
            if value.simplify() {
                assert!(value.complicate());
                assert_eq!(orig, value.current());
            }
        }
    }

    #[test]
    fn sampled_selects_correct_sizes_and_bits() {
        let input = u32::sampled(4..8, 10..20);
        let mut seen_counts = [0; 32];
        let mut seen_bits = [0; 32];

        let mut runner = TestRunner::deterministic();
        for _ in 0..2048 {
            let value = input.new_tree(&mut runner).unwrap().current();
            let count = value.count_ones() as usize;
            assert!(count >= 4 && count < 8);
            seen_counts[count] += 1;

            for bit in 0..32 {
                if 0 != value & (1 << bit) {
                    assert!(bit >= 10 && bit < 20);
                    seen_bits[bit] += value;
                }
            }
        }

        for i in 4..8 {
            assert!(seen_counts[i] >= 256 && seen_counts[i] < 1024);
        }

        let least_seen_bit_count =
            seen_bits[10..20].iter().cloned().min().unwrap();
        let most_seen_bit_count =
            seen_bits[10..20].iter().cloned().max().unwrap();
        assert_eq!(1, most_seen_bit_count / least_seen_bit_count);
    }

    #[test]
    fn sampled_doesnt_shrink_below_min_size() {
        let input = u32::sampled(4..8, 10..20);

        let mut runner = TestRunner::default();
        for _ in 0..256 {
            let mut value = input.new_tree(&mut runner).unwrap();
            while value.simplify() {}

            assert_eq!(4, value.current().count_ones());
        }
    }

    #[test]
    fn test_sanity() {
        check_strategy_sanity(u32::masked(0xdeadbeef), None);
    }
}
