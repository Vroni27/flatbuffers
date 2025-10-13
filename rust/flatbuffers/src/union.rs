/*
 * Copyright 2021 Google Inc. All rights reserved.
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

use crate::primitives::*;
use crate::{Push, Vector};

/// A trait defining TaggedUnion interface
pub trait TaggedUnion {
    type Tag: Push<Output = Self::Tag> + Clone + Copy + Into<u8>;
}

/// UnionWIPOffset marks a `WIPOffset` as being for a union value.
pub struct UnionWIPOffset<T: TaggedUnion> {
    tag: T::Tag,
    value_offset: WIPOffset<T>,
}

impl<T: TaggedUnion> UnionWIPOffset<T> {
    #[inline]
    pub fn new(tag: T::Tag, value_offset: WIPOffset<T>) -> Self {
        Self { tag, value_offset }
    }

    #[inline]
    pub fn tag(&self) -> T::Tag {
        self.tag
    }

    #[inline]
    pub fn value_offset(&self) -> WIPOffset<T> {
        self.value_offset
    }
}

/// Combines a vector of unions' values offsets with a vector
/// of their associated tags (discriminants) values
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct UnionVectorWIPOffsets<'a, T: TaggedUnion> {
    tags: WIPOffset<Vector<'a, T::Tag>>,
    values_offset: WIPOffset<Vector<'a, ForwardsUOffset<T>>>,
}

impl<'a, T: TaggedUnion> UnionVectorWIPOffsets<'a, T> {
    #[inline]
    pub fn new(
        tags: WIPOffset<Vector<'a, T::Tag>>,
        values_offset: WIPOffset<Vector<'a, ForwardsUOffset<T>>>,
    ) -> Self {
        Self {
            tags,
            values_offset,
        }
    }

    #[inline]
    pub fn tags(&self) -> WIPOffset<Vector<'a, T::Tag>> {
        self.tags
    }

    #[inline]
    pub fn values_offset(&self) -> WIPOffset<Vector<'a, ForwardsUOffset<T>>> {
        self.values_offset
    }
}
