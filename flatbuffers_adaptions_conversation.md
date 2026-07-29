# Flatbuffers adaptions conversation

User: The FlatBuffers compiler for Rust generates methods that require a reference to a `u8` array (`&[u8]`). What would be necessary to change so I can provide a trait with explicit offsets and lengths instead? The reason I think about this option is to implement a User Space Pager to optimize the Memory Management in my application.

Response:

Now I have enough context to give a comprehensive answer. Here's what's involved at each layer of the stack:

---

## The Problem in One Sentence

The entire FlatBuffers Rust runtime assumes a single contiguous `&[u8]` slice. Every byte read, offset traversal, and accessor flows through this assumption — so supporting a paged (non-contiguous, on-demand) buffer requires changes at multiple layers.

---

## What Drives the `&[u8]` Assumption

### Layer 1 — `Follow` trait (follow.rs)
```rust
pub trait Follow<'buf> {
    type Inner;
    unsafe fn follow(buf: &'buf [u8], loc: usize) -> Self::Inner;
}
```
This is the **root cause**. Every generated type implements `Follow` with a concrete `&[u8]`. Offset arithmetic works by doing `&buf[loc..]` directly.

### Layer 2 — `Table` struct (table.rs)
```rust
pub struct Table<'a> {
    buf: &'a [u8],
    loc: usize,
}
```
Stores the buffer reference directly; every field accessor calls `T::follow(self.buf, self.loc + o)`.

### Layer 3 — Scalar reads (endian_scalar.rs)
```rust
pub unsafe fn read_scalar_at<T: EndianScalar>(s: &[u8], loc: usize) -> T {
    read_scalar(&s[loc..])
}
```
Does a raw `ptr::copy_nonoverlapping` from the slice. Requires contiguous bytes.

### Layer 4 — `Verifier` (verifier.rs)
Also holds `buffer: &[u8]` directly and bounds-checks ranges against it.

### Layer 5 — Generated entry points (idl_gen_rust.cpp)
```cpp
code_ += "pub fn root_as_{{STRUCT_FN}}(buf: &[u8]) ...";
```
Every `root_as_*` function and the generated `Follow` impls are hardcoded to `&[u8]`.

---

## What You'd Need to Change

### Step 1 — Define a `ReadBuffer` trait

```rust
pub unsafe trait ReadBuffer {
    /// Returns a pointer to at least `len` bytes starting at `offset`.
    /// The implementation is responsible for paging in data and keeping
    /// the backing memory pinned for the duration of `'_`.
    unsafe fn read_bytes(&self, offset: usize, len: usize) -> *const u8;
}

// Blanket impl so existing code keeps working
unsafe impl ReadBuffer for [u8] {
    unsafe fn read_bytes(&self, offset: usize, len: usize) -> *const u8 {
        self[offset..offset + len].as_ptr()
    }
}
```

Using a raw pointer return (instead of `&[u8]`) is the key: it lets the pager control the backing lifetime and evict pages independently of Rust's borrow checker. `unsafe` on the trait enforces that implementors uphold the contract.

### Step 2 — Parameterize `Follow`, `Table`, `VTable`, `Vector`

```rust
pub trait Follow<'buf, B: ReadBuffer + ?Sized = [u8]> {
    type Inner;
    unsafe fn follow(buf: &'buf B, loc: usize) -> Self::Inner;
}

pub struct Table<'a, B: ReadBuffer + ?Sized = [u8]> {
    buf: &'a B,
    loc: usize,
}
```

The `= [u8]` default keeps all existing call sites compiling without change (backwards compatible).

### Step 3 — Update `read_scalar_at` / `read_scalar`

```rust
pub unsafe fn read_scalar_at<T: EndianScalar, B: ReadBuffer + ?Sized>(
    buf: &B, loc: usize
) -> T {
    let size = size_of::<T::Scalar>();
    let ptr = unsafe { buf.read_bytes(loc, size) };
    let mut mem = MaybeUninit::<T::Scalar>::uninit();
    ptr::copy_nonoverlapping(ptr, mem.as_mut_ptr() as *mut u8, size);
    T::from_little_endian(mem.assume_init())
}
```

### Step 4 — Update all `Follow` impls in primitives.rs, vtable.rs, vector.rs

Each of the ~15 `Follow` impls needs the generic `B` parameter threaded through.

### Step 5 — Update `Verifier`

The verifier also needs to become generic over `B: ReadBuffer`, since it performs bounds-checked reads. This is the most complex part: the current verifier uses Rust slice indexing for all its safety checks.

### Step 6 — Update idl_gen_rust.cpp

The code generator's generated signatures:
```cpp
// Before
code_ += "pub fn root_as_{{STRUCT_FN}}(buf: &[u8]) ...";

// After — add generic, keep [u8] default so existing callers work
code_ += "pub fn root_as_{{STRUCT_FN}}<B: flatbuffers::ReadBuffer + ?Sized>"
         "(buf: &B) ...";
```

---

## Practical Recommendation

Before implementing the above, consider whether you can implement the pager **below** the FlatBuffers API using OS primitives:

| Approach | Complexity | FlatBuffers changes |
|---|---|---|
| `mmap` + `madvise(MADV_DONTNEED)` | Low | None — OS handles paging |
| Linux `userfaultfd` | Medium | None — transparent to userspace |
| Generic `ReadBuffer` trait (above) | High | ~6 files + codegen |

If the data is file-backed (e.g. a large `.bin`), `mmap` gives you demand-paging and LRU eviction for free. FlatBuffers sees a plain `&[u8]` and the kernel does the work. The generic-buffer approach is only necessary if you need to page from a source that the kernel cannot directly address (e.g., a network block device in pure userspace, or RDMA memory).

User: &str expects a static memory address to my knowledge. Does this proposal address this issue?

GitHub Copilot: `&str` does **not** require a `'static` address — it only requires the bytes to remain at a **stable address for the duration of the borrow lifetime** `'a`. The issue is subtler.

## The Actual Problem: Page Eviction vs. Live References

The proposal's `ReadBuffer` trait returns a raw `*const u8`:

```rust
unsafe fn read_bytes(&self, offset: usize, len: usize) -> *const u8;
```

This deliberately bypasses Rust's lifetime tracking. But FlatBuffers' generated string accessors produce **references** that re-enter the lifetime system:

```rust
// How strings are followed today (primitives.rs)
impl<'a> Follow<'a> for &'a str {
    type Inner = &'a str;
    unsafe fn follow(buf: &'a [u8], loc: usize) -> Self::Inner {
        // returns a &str whose lifetime is tied to buf
    }
}
```

After threading `B: ReadBuffer` through, this becomes:

```rust
unsafe fn follow(buf: &'a B, loc: usize) -> &'a str { ... }
```

The `&'a str` lifetime is tied to `buf`, **not to any individual page**. If the pager evicts the page backing that string while `'a` is still live, the `&str` becomes a dangling pointer — **undefined behavior** that `unsafe` on `ReadBuffer` is supposed to prevent but cannot enforce by itself.

## What the Proposal Misses

The proposal handles **scalar fields** correctly — `read_scalar` copies the value out, so there's no dangling-reference risk:

```
i32, u64, f32, bool → copied out → safe after page eviction ✓
```

But **reference-returning fields** are still broken:

```
&str, &[u8], Vector<T>, Table<T> → borrow into page → unsafe if page evicts ✗
```

## The Fix: Require Page Pinning in the Contract

The `ReadBuffer` trait needs a pinning mechanism:

```rust
pub unsafe trait ReadBuffer {
    /// A guard that keeps the page pinned until dropped.
    type PageGuard<'a>: Deref<Target = [u8]> where Self: 'a;

    /// Returns a guard that pins the page at [offset, offset+len).
    /// The bytes remain at a stable address until the guard is dropped.
    fn pin_bytes(&self, offset: usize, len: usize) -> Self::PageGuard<'_>;
}
```

For the `[u8]` blanket impl, `PageGuard` is just `&[u8]` (a no-op). For a real pager:

```rust
pub struct PinnedPage<'a> {
    ptr: *const u8,
    len: usize,
    pager: &'a MyPager, // holds a refcount / pin ticket
}
impl Drop for PinnedPage<'_> {
    fn drop(&mut self) { self.pager.unpin(...); }
}
impl Deref for PinnedPage<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] { unsafe { slice::from_raw_parts(self.ptr, self.len) } }
}
```

The consequence is that **string and byte-slice accessors must return owned types** (`String`, `Cow<str>`, `Vec<u8>`) or carry the guard — because the reference lifetime must not outlive the pin. This means further changes to the generated `Follow::Inner` associated types for reference-typed fields, which propagates into the codegen in idl_gen_rust.cpp.

In short: the original proposal is complete for scalar/struct fields, but **reference-type fields require page pinning** to be safe with a user-space pager.