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

/// Read-only byte access used by every FlatBuffers read.
///
/// The default backing store is a plain `[u8]` slice. Implementing
/// `ReadBuffer` lets a wrapper observe every access the FlatBuffers runtime
/// and generated code make into the buffer — e.g. to record page hits for a
/// mmap/userfaultfd-based pager, where resident-page reads are otherwise
/// invisible.
///
/// All reads (scalars, offsets, vtables, strings, byte vectors, structs and
/// inline arrays) go through [`bytes`][ReadBuffer::bytes].
///
/// # Example — page-hit tracking wrapper
///
/// ```rust,ignore
/// use flatbuffers::ReadBuffer;
///
/// pub struct TracedBuf<'m> { data: &'m [u8], /* hit recorder */ }
///
/// unsafe impl ReadBuffer for TracedBuf<'_> {
///     unsafe fn bytes(&self, offset: usize, len: usize) -> &[u8] {
///         // record_hit(offset / PAGE_SIZE ..= (offset + len - 1) / PAGE_SIZE);
///         &self.data[offset..offset + len]
///     }
///     fn len(&self) -> usize { self.data.len() }
/// }
/// ```
///
/// # Safety
///
/// Implementors must guarantee that the bytes returned by `bytes` are
/// initialised, exactly `len` long, and stay valid at the same address for
/// the whole lifetime of the `&self` borrow. A mmap/userfaultfd mapping
/// satisfies this naturally: its virtual address never changes, and evicted
/// pages are transparently faulted back in on the next access.
pub unsafe trait ReadBuffer {
    /// Returns the byte range `[offset, offset + len)`.
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
}

/// Zero-cost implementation for plain byte slices.
// SAFETY: a `[u8]` sub-slice is initialised, correctly sized and stable for
// the borrow.
unsafe impl ReadBuffer for [u8] {
    #[inline]
    unsafe fn bytes(&self, offset: usize, len: usize) -> &[u8] {
        &self[offset..offset + len]
    }

    #[inline]
    fn len(&self) -> usize {
        <[u8]>::len(self)
    }
}
