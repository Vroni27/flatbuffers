/*
 * Copyright 2018 Google Inc. All rights reserved.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use core::cmp::Ordering;
use core::fmt::{Debug, Formatter, Result};
use core::iter::{DoubleEndedIterator, ExactSizeIterator, FusedIterator};
use core::marker::PhantomData;
use core::mem::{align_of, size_of};
use core::str::from_utf8_unchecked;

use crate::endian_scalar::read_scalar_at;
use crate::follow::Follow;
use crate::primitives::*;
use crate::read_buffer::ReadBuffer;

pub struct Vector<'a, T: 'a, B: ReadBuffer + ?Sized = [u8]>(&'a B, usize, PhantomData<T>);

impl<'a, T: 'a> Default for Vector<'a, T, [u8]> {
    fn default() -> Self {
        // Static, length 0 vector.
        // Note that derived default causes UB due to issues in read_scalar_at /facepalm.
        Self(&[0; core::mem::size_of::<UOffsetT>()], 0, Default::default())
    }
}

impl<'a, T, B: ReadBuffer + ?Sized> Debug for Vector<'a, T, B>
where
    T: 'a + Follow<'a, B>,
    <T as Follow<'a, B>>::Inner: Debug,
{
    fn fmt(&self, f: &mut Formatter) -> Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

// We cannot use derive for these two impls, as it would only implement Copy
// and Clone for `T: Copy` and `T: Clone` respectively. However `Vector<'a, T>`
// can always be copied, no matter that `T` you have.
impl<'a, T, B: ReadBuffer + ?Sized> Copy for Vector<'a, T, B> {}

impl<'a, T, B: ReadBuffer + ?Sized> Clone for Vector<'a, T, B> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, T: 'a, B: ReadBuffer + ?Sized> Vector<'a, T, B> {
    /// # Safety
    ///
    /// `buf` contains a valid vector at `loc` consisting of
    ///
    /// - UOffsetT element count
    /// - Consecutive list of `T` elements
    #[inline(always)]
    pub unsafe fn new(buf: &'a B, loc: usize) -> Self {
        Vector(buf, loc, PhantomData)
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        // Safety:
        // Valid vector at time of construction starting with UOffsetT element count
        unsafe { read_scalar_at::<UOffsetT, B>(self.0, self.1) as usize }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<'a, T: 'a> Vector<'a, T, [u8]> {
    #[inline(always)]
    pub fn bytes(&self) -> &'a [u8] {
        let sz = size_of::<T>();
        let len = self.len();
        &self.0[self.1 + SIZE_UOFFSET..self.1 + SIZE_UOFFSET + sz * len]
    }
}

impl<'a, T: Follow<'a, B> + 'a, B: ReadBuffer + ?Sized> Vector<'a, T, B> {
    #[inline(always)]
    pub fn get(&self, idx: usize) -> <T as Follow<'a, B>>::Inner {
        assert!(idx < self.len());
        let sz = size_of::<T>();
        debug_assert!(sz > 0);
        // Safety:
        // Valid vector at time of construction, verified that idx < element count
        unsafe { T::follow(self.0, self.1 as usize + SIZE_UOFFSET + sz * idx) }
    }

    #[inline(always)]
    pub fn lookup_by_key<K: Ord>(
        &self,
        key: K,
        f: fn(&<T as Follow<'a, B>>::Inner, &K) -> Ordering,
    ) -> Option<<T as Follow<'a, B>>::Inner> {
        if self.is_empty() {
            return None;
        }

        let mut left: usize = 0;
        let mut right = self.len() - 1;

        while left <= right {
            let mid = (left + right) / 2;
            let value = self.get(mid);
            match f(&value, &key) {
                Ordering::Equal => return Some(value),
                Ordering::Less => left = mid + 1,
                Ordering::Greater => {
                    if mid == 0 {
                        return None;
                    }
                    right = mid - 1;
                }
            }
        }

        None
    }

    #[inline(always)]
    pub fn iter(&self) -> VectorIter<'a, T, B> {
        VectorIter::from_vector(*self)
    }
}

/// # Safety
///
/// `buf` must contain a value of T at `loc` and have alignment of 1.
/// For a pager, the page must remain pinned for the lifetime `'a`.
pub unsafe fn follow_cast_ref<'a, T: Sized + 'a, B: ReadBuffer + ?Sized>(
    buf: &'a B,
    loc: usize,
) -> &'a T {
    assert_eq!(align_of::<T>(), 1);
    let sz = size_of::<T>();
    // Safety: caller guarantees T is at loc with alignment 1; bytes() ties lifetime to 'a.
    let slice = unsafe { buf.bytes(loc, sz) };
    let ptr = slice.as_ptr() as *const T;
    // SAFETY: buf contains a value at loc of type T; T has alignment 1.
    &*ptr
}

impl<'a, B: ReadBuffer + ?Sized> Follow<'a, B> for &'a str {
    type Inner = &'a str;
    unsafe fn follow(buf: &'a B, loc: usize) -> Self::Inner {
        let len = read_scalar_at::<UOffsetT, B>(buf, loc) as usize;
        let slice = buf.bytes(loc + SIZE_UOFFSET, len);
        from_utf8_unchecked(slice)
    }
}

impl<'a, B: ReadBuffer + ?Sized> Follow<'a, B> for &'a [u8] {
    type Inner = &'a [u8];
    unsafe fn follow(buf: &'a B, loc: usize) -> Self::Inner {
        let len = read_scalar_at::<UOffsetT, B>(buf, loc) as usize;
        buf.bytes(loc + SIZE_UOFFSET, len)
    }
}

/// Implement Follow for all possible Vectors that have Follow-able elements.
///
/// The vector struct itself stores `&'a [u8]` (obtained via
/// [`ReadBuffer::bytes`]) so that its internal element accessors keep working
/// unchanged regardless of the backing buffer type.
impl<'a, T: Follow<'a, B> + 'a, B: ReadBuffer + ?Sized> Follow<'a, B> for Vector<'a, T, B> {
    type Inner = Vector<'a, T, B>;
    unsafe fn follow(buf: &'a B, loc: usize) -> Self::Inner {
        Vector::new(buf, loc)
    }
}

/// An iterator over a `Vector`.
#[derive(Debug)]
pub struct VectorIter<'a, T: 'a, B: ReadBuffer + ?Sized = [u8]> {
    buf: &'a B,
    loc: usize,
    remaining: usize,
    phantom: PhantomData<T>,
}

impl<'a, T: 'a, B: ReadBuffer + ?Sized> VectorIter<'a, T, B> {
    #[inline]
    pub fn from_vector(inner: Vector<'a, T, B>) -> Self {
        VectorIter {
            buf: inner.0,
            // inner.1 is the location of the data for the vector.
            // The first SIZE_UOFFSET bytes is the length. We skip
            // that to get to the actual vector content.
            loc: inner.1 + SIZE_UOFFSET,
            remaining: inner.len(),
            phantom: PhantomData,
        }
    }
}

impl<'a, T: 'a> VectorIter<'a, T, [u8]> {
    /// Creates a new `VectorIter` from the provided slice
    ///
    /// # Safety
    ///
    /// buf must contain a contiguous sequence of `items_num` values of `T`
    ///
    #[inline]
    pub unsafe fn from_slice(buf: &'a [u8], items_num: usize) -> Self {
        VectorIter { buf, loc: 0, remaining: items_num, phantom: PhantomData }
    }
}

impl<'a, T: Follow<'a, B> + 'a, B: ReadBuffer + ?Sized> Clone for VectorIter<'a, T, B> {
    #[inline]
    fn clone(&self) -> Self {
        VectorIter {
            buf: self.buf,
            loc: self.loc,
            remaining: self.remaining,
            phantom: self.phantom,
        }
    }
}

impl<'a, T: Follow<'a, B> + 'a, B: ReadBuffer + ?Sized> Iterator for VectorIter<'a, T, B> {
    type Item = <T as Follow<'a, B>>::Inner;

    #[inline]
    fn next(&mut self) -> Option<<T as Follow<'a, B>>::Inner> {
        let sz = size_of::<T>();
        debug_assert!(sz > 0);

        if self.remaining == 0 {
            None
        } else {
            // Safety:
            // VectorIter can only be created from a contiguous sequence of `items_num`
            // And remaining is initialized to `items_num`
            let result = unsafe { T::follow(self.buf, self.loc) };
            self.loc += sz;
            self.remaining -= 1;
            Some(result)
        }
    }

    #[inline]
    fn nth(&mut self, n: usize) -> Option<<T as Follow<'a, B>>::Inner> {
        let sz = size_of::<T>();
        debug_assert!(sz > 0);

        self.remaining = self.remaining.saturating_sub(n);

        // Note that this might overflow, but that is okay because
        // in that case self.remaining will have been set to zero.
        self.loc = self.loc.wrapping_add(sz * n);

        self.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a, T: Follow<'a, B> + 'a, B: ReadBuffer + ?Sized> DoubleEndedIterator for VectorIter<'a, T, B> {
    #[inline]
    fn next_back(&mut self) -> Option<<T as Follow<'a, B>>::Inner> {
        let sz = size_of::<T>();
        debug_assert!(sz > 0);

        if self.remaining == 0 {
            None
        } else {
            self.remaining -= 1;
            // Safety:
            // VectorIter can only be created from a contiguous sequence of `items_num`
            // And remaining is initialized to `items_num`
            Some(unsafe { T::follow(self.buf, self.loc + sz * self.remaining) })
        }
    }

    #[inline]
    fn nth_back(&mut self, n: usize) -> Option<<T as Follow<'a, B>>::Inner> {
        self.remaining = self.remaining.saturating_sub(n);
        self.next_back()
    }
}

impl<'a, T: 'a + Follow<'a, B>, B: ReadBuffer + ?Sized> ExactSizeIterator for VectorIter<'a, T, B> {
    #[inline]
    fn len(&self) -> usize {
        self.remaining
    }
}

impl<'a, T: 'a + Follow<'a, B>, B: ReadBuffer + ?Sized> FusedIterator for VectorIter<'a, T, B> {}

impl<'a, T: Follow<'a, B> + 'a, B: ReadBuffer + ?Sized> IntoIterator for Vector<'a, T, B> {
    type Item = <T as Follow<'a, B>>::Inner;
    type IntoIter = VectorIter<'a, T, B>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, 'b, T: Follow<'a, B> + 'a, B: ReadBuffer + ?Sized> IntoIterator for &'b Vector<'a, T, B> {
    type Item = <T as Follow<'a, B>>::Inner;
    type IntoIter = VectorIter<'a, T, B>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(feature = "serialize")]
impl<'a, T, B: ReadBuffer + ?Sized> serde::ser::Serialize for Vector<'a, T, B>
where
    T: 'a + Follow<'a, B>,
    <T as Follow<'a, B>>::Inner: serde::ser::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for element in self {
            seq.serialize_element(&element)?;
        }
        seq.end()
    }
}
