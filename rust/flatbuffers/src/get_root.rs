/*
 * Copyright 2020 Google Inc. All rights reserved.
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

use crate::{
    Follow, ForwardsUOffset, InvalidFlatbuffer, ReadBuffer, SkipSizePrefix, Verifiable, Verifier,
    VerifierOptions,
};

/// Gets the root of the Flatbuffer, verifying it first with default options.
/// Note that verification is an experimental feature and may not be maximally performant or
/// catch every error (though that is the goal). See the `_unchecked` variants for previous
/// behavior.
pub fn root<'buf, T>(data: &'buf [u8]) -> Result<T::Inner, InvalidFlatbuffer>
where
    T: 'buf + Follow<'buf> + Verifiable,
{
    let opts = VerifierOptions::default();
    root_with_opts::<T>(&opts, data)
}

#[inline]
/// Gets the root of the Flatbuffer, verifying it first with given options.
/// Note that verification is an experimental feature and may not be maximally performant or
/// catch every error (though that is the goal). See the `_unchecked` variants for previous
/// behavior.
pub fn root_with_opts<'opts, 'buf, T>(
    opts: &'opts VerifierOptions,
    data: &'buf [u8],
) -> Result<T::Inner, InvalidFlatbuffer>
where
    T: 'buf + Follow<'buf> + Verifiable,
{
    let mut v = Verifier::new(opts, data);
    <ForwardsUOffset<T>>::run_verifier(&mut v, 0)?;
    // Safety:
    // Run verifier above
    Ok(unsafe { root_unchecked::<T, [u8]>(data) })
}

#[inline]
/// Gets the root of a size prefixed Flatbuffer, verifying it first with default options.
/// Note that verification is an experimental feature and may not be maximally performant or
/// catch every error (though that is the goal). See the `_unchecked` variants for previous
/// behavior.
pub fn size_prefixed_root<'buf, T>(data: &'buf [u8]) -> Result<T::Inner, InvalidFlatbuffer>
where
    T: 'buf + Follow<'buf> + Verifiable,
{
    let opts = VerifierOptions::default();
    size_prefixed_root_with_opts::<T>(&opts, data)
}

#[inline]
/// Gets the root of a size prefixed Flatbuffer, verifying it first with given options.
/// Note that verification is an experimental feature and may not be maximally performant or
/// catch every error (though that is the goal). See the `_unchecked` variants for previous
/// behavior.
pub fn size_prefixed_root_with_opts<'opts, 'buf, T>(
    opts: &'opts VerifierOptions,
    data: &'buf [u8],
) -> Result<T::Inner, InvalidFlatbuffer>
where
    T: 'buf + Follow<'buf> + Verifiable,
{
    let mut v = Verifier::new(opts, data);
    <SkipSizePrefix<ForwardsUOffset<T>>>::run_verifier(&mut v, 0)?;
    // Safety:
    // Run verifier above
    Ok(unsafe { size_prefixed_root_unchecked::<T, [u8]>(data) })
}

#[inline]
/// Gets root for a trusted Flatbuffer.
/// # Safety
/// Flatbuffers accessors do not perform validation checks before accessing. Unlike the other
/// `root` functions, this does not validate the flatbuffer before returning the accessor. Users
/// must trust `data` contains a valid flatbuffer (e.g. b/c it was built by your software). Reading
/// unchecked buffers may cause panics or even UB.
pub unsafe fn root_unchecked<'buf, T, B: ReadBuffer + ?Sized>(data: &'buf B) -> T::Inner
where
    T: Follow<'buf, B> + 'buf,
{
    <ForwardsUOffset<T>>::follow(data, 0)
}

#[inline]
/// Gets root for a trusted, size prefixed, Flatbuffer.
/// # Safety
/// Flatbuffers accessors do not perform validation checks before accessing. Unlike the other
/// `root` functions, this does not validate the flatbuffer before returning the accessor. Users
/// must trust `data` contains a valid flatbuffer (e.g. b/c it was built by your software). Reading
/// unchecked buffers may cause panics or even UB.
pub unsafe fn size_prefixed_root_unchecked<'buf, T, B: ReadBuffer + ?Sized>(data: &'buf B) -> T::Inner
where
    T: Follow<'buf, B> + 'buf,
{
    <SkipSizePrefix<ForwardsUOffset<T>>>::follow(data, 0)
}

// ── Generic (ReadBuffer) verified entry points ─────────────────────────────
//
// These functions accept any `B: ReadBuffer` backing store. They obtain a
// `&'buf [u8]` view of the whole buffer once (pinning all pages for `'buf`)
// and run the existing `[u8]`-based verifier against it, then return the root
// object accessed through the generic buffer.
//
// Verification necessarily reads every byte, so pinning all pages is optimal.

#[inline]
/// Verifies a [`ReadBuffer`]-backed FlatBuffer and returns its root.
///
/// Equivalent to [`root`] but works with any buffer type implementing
/// [`ReadBuffer`], including user-space pagers.
///
/// # Safety (inner `unsafe` block)
/// [`ReadBuffer::bytes`] is called once to obtain `&'buf [u8]`; the caller
/// must uphold the [`ReadBuffer`] contract (bytes stable for `'buf`).
pub fn root_with_buffer<'buf, T, B>(
    data: &'buf B,
) -> Result<T::Inner, InvalidFlatbuffer>
where
    B: ReadBuffer + ?Sized,
    T: 'buf + Follow<'buf, B> + Verifiable,
{
    let opts = VerifierOptions::default();
    root_with_buffer_and_opts::<T, B>(&opts, data)
}

#[inline]
/// Verifies a [`ReadBuffer`]-backed FlatBuffer with custom options and returns its root.
pub fn root_with_buffer_and_opts<'opts, 'buf, T, B>(
    opts: &'opts VerifierOptions,
    data: &'buf B,
) -> Result<T::Inner, InvalidFlatbuffer>
where
    B: ReadBuffer + ?Sized,
    T: 'buf + Follow<'buf, B> + Verifiable,
{
    // SAFETY: ReadBuffer contract guarantees bytes valid for 'buf.
    let mut v = unsafe { Verifier::new_with_buffer(opts, data) };
    <ForwardsUOffset<T>>::run_verifier(&mut v, 0)?;
    Ok(unsafe { root_unchecked::<T, B>(data) })
}

#[inline]
/// Verifies a size-prefixed [`ReadBuffer`]-backed FlatBuffer and returns its root.
pub fn size_prefixed_root_with_buffer<'buf, T, B>(
    data: &'buf B,
) -> Result<T::Inner, InvalidFlatbuffer>
where
    B: ReadBuffer + ?Sized,
    T: 'buf + Follow<'buf, B> + Verifiable,
{
    let opts = VerifierOptions::default();
    size_prefixed_root_with_buffer_and_opts::<T, B>(&opts, data)
}

#[inline]
/// Verifies a size-prefixed [`ReadBuffer`]-backed FlatBuffer with custom options and returns its root.
pub fn size_prefixed_root_with_buffer_and_opts<'opts, 'buf, T, B>(
    opts: &'opts VerifierOptions,
    data: &'buf B,
) -> Result<T::Inner, InvalidFlatbuffer>
where
    B: ReadBuffer + ?Sized,
    T: 'buf + Follow<'buf, B> + Verifiable,
{
    // SAFETY: ReadBuffer contract guarantees bytes valid for 'buf.
    let mut v = unsafe { Verifier::new_with_buffer(opts, data) };
    <SkipSizePrefix<ForwardsUOffset<T>>>::run_verifier(&mut v, 0)?;
    Ok(unsafe { size_prefixed_root_unchecked::<T, B>(data) })
}
