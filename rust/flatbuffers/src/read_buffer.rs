/*
 * Copyright 2024 Google Inc. All rights reserved.
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

use core::ops::Deref;

/// Provides pinned read-only access to a region of a byte buffer.
///
/// FlatBuffers only needs to *read* bytes at arbitrary offsets. The default
/// backing store is a plain `&[u8]` slice. Implementing `ReadBuffer` lets
/// alternative stores — such as a **user-space pager** with non-contiguous or
/// demand-loaded pages — integrate with the FlatBuffers runtime.
///
/// # Design
///
/// Traversing a FlatBuffer follows chains of relative offsets, and returning
/// references (e.g. `&str`, `&[u8]`) into the buffer requires those bytes to
/// remain at a **stable virtual address** for the duration of the reference.
/// To satisfy this requirement without forcing all pages to be resident at
/// once, access is mediated through [`ReadBuffer::PageGuard`]:
///
/// - Call [`pin_bytes`][ReadBuffer::pin_bytes] to obtain a guard.
/// - The guard keeps the addressed region pinned and stable while it lives.
/// - Dropping the guard releases the pin; the pager may then evict the page.
///
/// For a plain `&[u8]`, the guard is a zero-cost sub-slice — no actual
/// pinning is needed because the memory is already stable.
///
/// # Safety
///
/// Implementors **must** uphold every guarantee listed below. Violating any
/// of them is **undefined behaviour**:
///
/// 1. **Stable address**: The bytes returned by `*guard` must reside at the
///    same virtual address for the entire lifetime of `guard`. The pager must
///    not move, remap, or reclaim those physical pages while the guard exists.
///
/// 2. **Correct length**: `(*guard).len() == len` for any call
///    `pin_bytes(offset, len)`.
///
/// 3. **Valid data**: Every byte in `*guard` must be initialised and readable
///    (i.e. no uninitialised memory exposed through the slice).
///
/// 4. **Bounds**: `offset + len <= self.len()`. Implementations may choose
///    to panic or exhibit defined-but-surprising behaviour outside this range;
///    they must not produce undefined behaviour.
///
/// # Example — plain `[u8]` (zero cost)
///
/// ```rust
/// use flatbuffers::ReadBuffer;
///
/// let data: &[u8] = b"\x04\x00\x00\x00hello";
/// let guard = data.pin_bytes(4, 5);
/// assert_eq!(*guard, *b"hello");
/// ```
///
/// # Example — skeleton of a user-space pager
///
/// ```rust,ignore
/// use core::ops::Deref;
/// use flatbuffers::ReadBuffer;
///
/// pub struct MyPager { /* ... */ }
///
/// /// Holds a refcount on one or more pages so the pager cannot evict them.
/// pub struct PinnedRegion<'pager> {
///     ptr: *const u8,
///     len: usize,
///     _pager: &'pager MyPager,   // lifetime keeps the pager alive
///     // store a ticket / pin handle here so Drop can unpin
/// }
///
/// impl Deref for PinnedRegion<'_> {
///     type Target = [u8];
///     fn deref(&self) -> &[u8] {
///         // SAFETY: ptr is valid and stable for our lifetime (pinned above).
///         unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
///     }
/// }
///
/// impl Drop for PinnedRegion<'_> {
///     fn drop(&mut self) {
///         // Release the pin so the pager may evict the page.
///         // self._pager.unpin(...)
///     }
/// }
///
/// unsafe impl ReadBuffer for MyPager {
///     type PageGuard<'a> = PinnedRegion<'a> where Self: 'a;
///
///     fn pin_bytes(&self, offset: usize, len: usize) -> PinnedRegion<'_> {
///         // 1. Ensure the page covering [offset, offset+len) is resident.
///         // 2. Increment its pin count.
///         // 3. Return the guard holding a raw pointer into the page.
///         todo!()
///     }
///
///     fn len(&self) -> usize {
///         todo!()
///     }
/// }
/// ```
pub unsafe trait ReadBuffer {
    /// A RAII guard that pins a contiguous byte region in memory.
    ///
    /// While this guard is alive:
    /// - the bytes it covers are at a **fixed virtual address**, and
    /// - dereferencing it yields exactly the `len` bytes requested.
    ///
    /// Dropping the guard signals to the implementor that the region no
    /// longer needs to be pinned.
    type PageGuard<'a>: Deref<Target = [u8]>
    where
        Self: 'a;

    /// Pins the byte range `[offset, offset + len)` and returns a guard
    /// keeping those bytes at a stable address.
    ///
    /// The guard **must** be kept alive for as long as any reference derived
    /// from its content is used (e.g. a `&str` or `&[u8]` pointing into it).
    ///
    /// # Panics
    ///
    /// Implementations may panic if `offset + len > self.len()`.
    fn pin_bytes(&self, offset: usize, len: usize) -> Self::PageGuard<'_>;

    /// Returns the total number of bytes in the buffer.
    fn len(&self) -> usize;

    /// Returns `true` if the buffer contains no bytes.
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Blanket implementation for plain byte slices.
///
/// [`pin_bytes`][ReadBuffer::pin_bytes] is a zero-cost sub-slice; there is
/// no actual pinning because `[u8]` is already resident in stable memory.
// SAFETY: `[u8]` is in stable memory by definition. The sub-slice has the
// correct length and all bytes are initialised (the slice invariant).
unsafe impl ReadBuffer for [u8] {
    type PageGuard<'a> = &'a [u8]
    where
        Self: 'a;

    #[inline]
    fn pin_bytes(&self, offset: usize, len: usize) -> &[u8] {
        &self[offset..offset + len]
    }

    #[inline]
    fn len(&self) -> usize {
        // Call the slice's inherent method explicitly to avoid ambiguity with
        // the trait method of the same name.
        <[u8]>::len(self)
    }
}
