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

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Read-only byte access used by FlatBuffers reads.
///
/// The default backing store is a plain `[u8]` slice. Implementing
/// `ReadBuffer` lets a wrapper observe the accesses the FlatBuffers runtime
/// and generated code make into the buffer, e.g. to record page hits for a
/// mmap/userfaultfd-based pager, where reads of resident pages are otherwise
/// invisible.
///
/// # Access granularity
///
/// Every scalar, offset and vtable read is one [`bytes`][ReadBuffer::bytes]
/// call. Reading one table field costs several calls (the table's vtable
/// offset, the vtable size, the field's vtable entry, then the value).
///
/// Accessors that hand out a borrowed slice or reference record **one** call
/// covering the whole range at the moment the accessor runs. Later reads
/// through the returned value are *not* observed. This applies to:
///
/// - strings (`&str`) and `[ubyte]`/`[byte]` vectors returned as `&[u8]`,
/// - [`Vector::bytes`][crate::Vector::bytes],
/// - structs (returned as `&T`) and the fixed-size arrays inside them,
/// - file identifiers.
///
/// Scalar vectors, vectors of tables/strings and nested flatbuffers are read
/// element by element through `bytes`.
///
/// Verification ([`crate::root`], [`crate::Verifier`]) works on `&[u8]` only
/// and is never observed.
///
/// # Example: page-hit tracking wrapper
///
/// ```rust,ignore
/// use flatbuffers::ReadBuffer;
///
/// pub struct TracedBuf<'m> { data: &'m [u8], /* hit recorder */ }
///
/// unsafe impl ReadBuffer for TracedBuf<'_> {
///     unsafe fn bytes(&self, offset: usize, len: usize) -> &[u8] {
///         if len != 0 {
///             // record_hit(offset / PAGE_SIZE ..= (offset + len - 1) / PAGE_SIZE);
///         }
///         &self.data[offset..offset + len]
///     }
///     fn len(&self) -> usize { self.data.len() }
/// }
/// ```
///
/// The hit is recorded *before* the caller touches the memory. A hit
/// recorder must therefore not hold any lock across the access that a
/// fault handler might need, and must not itself access the managed region.
///
/// # Safety
///
/// For the whole lifetime of the `&self` borrow, implementors must guarantee
/// that the slice returned by `bytes(offset, len)`:
///
/// 1. is exactly `len` bytes long and initialised,
/// 2. has **contents** that never change: every call for the same range
///    returns identical bytes, and those bytes are identical to the bytes that
///    were verified (if the buffer was verified). Rust treats the returned
///    `&[u8]` as immutable memory, so the compiler may cache or duplicate loads
///    from it,
/// 3. stays readable at the same address. A read may block, but it must never
///    observe zeroed, partially written or different data.
///
/// Rule 2 is about *contents*, not just addresses. A pager that evicts and
/// reloads pages only meets it if the reload is byte-identical. For a
/// userfaultfd pager that means all of the following must hold:
///
/// - the region is `MAP_PRIVATE | MAP_ANONYMOUS`, page-aligned in base and
///   length, and registered with `UFFDIO_REGISTER_MODE_MISSING`. On file-backed
///   or shmem mappings `MADV_DONTNEED` does not cause a missing fault; the page
///   comes back from the file or page cache instead,
/// - transparent huge pages are disabled for the region (`MADV_NOHUGEPAGE`),
/// - the process does not `fork()` while the region is in use, unless
///   `UFFD_FEATURE_EVENT_FORK` is handled. A child would otherwise see zero
///   pages,
/// - every fault is resolved with a full, byte-identical `UFFDIO_COPY`. Never
///   use `UFFDIO_ZEROPAGE`, a short read or a partial copy; treat `EEXIST`
///   as success,
/// - the backing storage is not modified while any borrow is alive,
/// - the fault-handler thread, and the eviction strategy it calls, never read
///   the managed region (directly, through a `ReadBuffer`, or via
///   `Debug`/logging of FlatBuffers values), or it deadlocks on its own fault.
///
/// If bounds checks are skipped in `bytes`, any out-of-range request from a
/// corrupt (unverified) buffer is undefined behaviour instead of a panic.
pub unsafe trait ReadBuffer {
    /// Returns the byte range `[offset, offset + len)`.
    ///
    /// `len` may be `0` (empty strings and vectors).
    ///
    /// # Safety
    ///
    /// Caller must ensure `offset + len <= self.len()`.
    ///
    /// # Panics
    ///
    /// May panic if `offset + len > self.len()`.
    unsafe fn bytes(&self, offset: usize, len: usize) -> &[u8];

    /// Returns the total number of bytes in the buffer.
    fn len(&self) -> usize;

    /// Returns `true` if the buffer contains no bytes.
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a buffer whose first 4 bytes are zero.
    ///
    /// It backs the default (empty) value of vector fields declared with
    /// `= []` in the schema when the field is absent. The default returns
    /// `None`; reading such an absent field then panics. Wrappers that are
    /// used with these schemas should override it, typically by returning a
    /// reference to a `static` instance that wraps `&[0u8; 4]`.
    #[inline]
    fn empty_vector<'a>() -> Option<&'a Self>
    where
        Self: 'a,
    {
        None
    }
}

static EMPTY_VECTOR: [u8; 4] = [0; 4];

/// Zero-cost implementation for plain byte slices.
// SAFETY: a `[u8]` sub-slice is initialised, correctly sized and immutable for
// the borrow.
unsafe impl ReadBuffer for [u8] {
    #[inline(always)]
    unsafe fn bytes(&self, offset: usize, len: usize) -> &[u8] {
        &self[offset..offset + len]
    }

    #[inline(always)]
    fn len(&self) -> usize {
        <[u8]>::len(self)
    }

    #[inline]
    fn empty_vector<'a>() -> Option<&'a Self>
    where
        Self: 'a,
    {
        Some(&EMPTY_VECTOR[..])
    }
}

// SAFETY: same as `[u8]`.
unsafe impl<const N: usize> ReadBuffer for [u8; N] {
    #[inline(always)]
    unsafe fn bytes(&self, offset: usize, len: usize) -> &[u8] {
        &self[offset..offset + len]
    }

    #[inline(always)]
    fn len(&self) -> usize {
        N
    }
}

// SAFETY: same as `[u8]`; the vector cannot be mutated while borrowed.
unsafe impl ReadBuffer for Vec<u8> {
    #[inline(always)]
    unsafe fn bytes(&self, offset: usize, len: usize) -> &[u8] {
        &self[offset..offset + len]
    }

    #[inline(always)]
    fn len(&self) -> usize {
        Vec::len(self)
    }
}
