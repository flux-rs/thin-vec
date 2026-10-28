# To file — thin-vec

Drafts, ready to paste, of everything Fluxing thin-vec found that isn't reported yet.
Checked against the upstream trackers on 2026-10-28. Background is in `BUGS.md` and
`FLUX_ISSUES.md`. Nothing here has been filed.

| # | where | what | route |
|---|---|---|---|
| T1 | mozilla/thin-vec | `ThinVec::<ZST>::insert` derefs the fake length pointer (segfault from safe code) | public issue (Firefox's `gecko-ffi` mode forbids ZSTs, #94), or a private report |
| T2 | mozilla/thin-vec | `drain` range-bound overflow drains the wrong range (release) | public issue |
| F1 | flux-rs/flux | No `T::size_of() >= 0` for generic `T` (also hit in ringbuf, where it was found first) | new issue |
| F3 | flux-rs/flux | Glob import before its module doesn't resolve in flux annotations | new issue |
| F4 | flux-rs/flux | No flux-core specs for `slice::from_raw_parts{,_mut}` / global `alloc` | new issue |
| F5 | flux-rs/flux #1715 | Smaller trigger for the `projections.rs:401` ICE | comment |

F-numbers match the shared flux list (F2, `NonZero`, is ringbuf-only).

**Not to file:**
- `FLUX_ISSUES.md` §3 (pointer `addr` not known `>= 0`): fixed by flux#1880 (merged 2026-10-21).
- ICE A (`TooGeneric`, #1800): flux#1873/#1874 (merged 2026-10-19) look like the fix; confirm
  on a newer driver, then drop the `ICE:` trust on `try_from` and comment on #1800.
- ICE B: #1821. ICE C: #1715 (see F5).
- flux#1761 (`slice as *const [T]` ICE, fixed by #1762) is **not** F4: it's the other direction and a crash.
  flux-core `main` still has no `slice::from_raw_parts` spec, only `NonNull::slice_from_raw_parts`
  (`lib/flux-core/src/ptr/non_null.rs`), which F4 can point to as the pattern.

All flux repros were run on `2cacd2ea`; re-run them on current `main` before posting.

---

## T1. `ThinVec::<ZST>::insert` dereferences the fake length pointer

**Title:** `ThinVec::<ZST>::insert` dereferences the fake length pointer (segfault from safe code)

**Body:**

Affects 0.2.20 and 0.2.21 (regression from #92, "Remove allocation for ZSTs").

Since #92, a ZST `ThinVec` has no allocation or header:
- `self.ptr` stores `len + 1` as its address (`len_to_ptr_unchecked`), and `len()` / `set_len_zst` decode it.
- Every length update goes through `set_len_non_singleton`, which dispatches to `set_len_zst` for ZSTs, **except `insert`**, which still ends with

```rust
self.set_header_len(old_len + 1);   // -> header_mut() -> &mut *self.ptr()
```

For a ZST, this forms a `&mut Header` at address `len + 1` and writes through it. Reproducer (no `unsafe`):

```rust
#![forbid(unsafe_code)]

fn main() {
    let mut v: thin_vec::ThinVec<()> = thin_vec::ThinVec::new();
    v.insert(0, ());
}
```

- release: **SIGSEGV**
- debug: panics with `assertion failed: !Self::is_zst()` (the `debug_assert!` in `header_mut`)
- Miri: `Undefined Behavior: constructing invalid value of type &mut thin_vec::Header: encountered a dangling reference (0x1[noalloc] has no provenance)`

For a vec of length `n`, the write goes to address `n + 1`, so it isn't limited to the null page. I ran every other public method on a ZST `ThinVec` under Miri, and only `insert` is affected.

**Fix:** in `insert`, replace `self.set_header_len(old_len + 1)` with `self.set_len_non_singleton(old_len + 1)`, as `push_unchecked` does.

Regression test:

```rust
#[test]
fn zst_insert() {
    let mut v: ThinVec<()> = ThinVec::new();
    v.insert(0, ());
    v.insert(0, ());
    assert_eq!(v.len(), 2);
}
```

---

## T2. `drain` range-bound overflow

**Title:** `drain` with `Excluded(usize::MAX)` start or `..=usize::MAX` end overflows and drains the wrong range in release

**Body:**

Affects 0.2.21 / `main`. `ThinVec::drain` computes its bounds as (abridged):

```rust
let start = match range.start_bound() { Bound::Excluded(&n) => n + 1, /* .. */ };
let end   = match range.end_bound()   { Bound::Included(&n) => n + 1, /* .. */ };
```

Neither `+ 1` is checked. `Vec::drain` (and thin-vec's own `extract_if`) panics for these ranges, but in release builds thin-vec wraps instead:

| call on `thin_vec![1, 2, 3]` | `Vec` | `ThinVec` (release) |
|---|---|---|
| `drain((Bound::Excluded(usize::MAX), Bound::Unbounded))` | panics | drains **all** elements `[1, 2, 3]` |
| `drain(..=usize::MAX)` | panics | drains **nothing** |

Debug builds panic with `attempt to add with overflow`. There's no UB: the wrapped bounds still pass `drain`'s `assert!(start <= end)` / `assert!(end <= len)`, so the wrong range is drained safely.

**Fix:** compute the range the way `extract_if` already does. Hoist its local `slice_range` / `slice_index_fail` (a backport of core's range handling) to module scope and use `let Range { start, end } = slice_range(range, ..len);` in `drain`. That rejects `Excluded(usize::MAX)` and `..=usize::MAX` before adding 1, with the same messages as `Vec`.

**Test** (fails under `cargo test --release`):

```rust
use core::ops::Bound;

#[test]
fn drain_excluded_usize_max_start() {
    let mut v = thin_vec![1, 2, 3];
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        v.drain((Bound::Excluded(usize::MAX), Bound::Unbounded)).count()
    }));
    assert!(r.is_err());
}
```

---

## F1. No `T::size_of() >= 0` for a generic `T`

**Found while Fluxing:** ringbuf (first), then thin-vec

**Title:** `valid(p, T::size_of())` unprovable for generic `T`: no `T::size_of() >= 0` axiom

**Body:**

`&T as *const T` is typed with `p.size == T::size_of()` (`raw_ptr_with_size` in
flux-refineck's `checker.rs`), and flux-core's `valid(p, n)` needs `p.size >= 0`.
With `T` generic, `T::size_of()` is an uninterpreted alias, so nothing gives
`p.size >= 0`. `ptr::read(x)` with `x: &T` verifies for every concrete `T`, but
never for a generic one:

```rust
extern crate flux_core;
use core::ptr;

pub fn a<T>(x: &T) -> T { unsafe { ptr::read(x) } }       // error: valid(p, T::size_of())
pub fn b(x: &u32) -> u32 { unsafe { ptr::read(x) } }      // ok

#[flux_rs::spec(fn(&T) -> T requires T::size_of() >= 0)]
pub fn c<T>(x: &T) -> T { unsafe { ptr::read(x) } }       // ok
```

It also blocks slice pointer arithmetic: a pointer from `<[T]>::as_mut_ptr` has
size `n * T::size_of()`, and `in_bounds(p)` / `i * T::size_of() <= n * T::size_of()`
for `i < n` both need `T::size_of() >= 0`. In thin-vec, adding
`requires T::size_of() >= 0` to `remove` took it from 9 errors to 0. But that
precondition can't go on a trait-method impl (e.g. `DoubleEndedIterator::next_back`),
so those stay unverified.

**Suggested fix:** assume `0 <= T::size_of()` (and `1 <= T::align_of()`) for every
`T`, e.g. in `builtin_assoc_reft_body`'s generic fallback or where the sized
alias is built.

**Hit in:** ringbuf (`{Local,Shared}Rb::into_raw_parts`, `Direct::into_rb_ref`),
thin-vec (`remove`, `swap_remove`, `dedup_by`, `pop`, `Drain::next_back`,
`header_with_capacity`).

---

## F3. Glob import before its module doesn't resolve in flux annotations

**Found while Fluxing:** thin-vec

**Title:** Name in a flux annotation not found when it comes from a glob import that precedes its module

**Body:**

```rust
extern crate flux_core;
use m::*;                         // glob import first ...
mod m { pub type Sz = usize; }    // ... module after
#[flux_rs::refined_by(n: int)]
pub struct H {
    #[field(Sz[n])]               // error[E0999]: cannot find type `Sz` in this scope
    pub x: Sz,                    // rustc resolves it fine
}
```

It works if the `mod` comes first, with a named import (`use m::Sz;`) in either
order, or with a path (`m::Sz`). It also fails on a driver from before #1857
("Resolve rust paths in flux definitions"), so it isn't a regression from that change.

**Hit in:** thin-vec, which has exactly this shape: `use impl_details::*;` at the top
of `lib.rs`, with the `cfg`'d `mod impl_details` below it. Worked around with the
qualified `impl_details::SizeType`.

---

## F4. No flux-core specs for `slice::from_raw_parts{,_mut}` or the global allocator

**Found while Fluxing:** thin-vec (ringbuf was affected the same way, noticed afterwards)

**Title:** Missing flux-core specs: `core::slice::from_raw_parts{,_mut}` (unchecked precondition, unknown length) and `alloc::alloc::{alloc, realloc, dealloc}`

**Body:**

`slice::from_raw_parts(p, n)` has no spec, so its safety precondition is never
checked and flux doesn't know the result has length `n`. Any accessor that builds a
slice from a raw pointer passes without being checked. In thin-vec, `as_slice`,
`as_mut_slice`, `Deref`/`DerefMut`, `IntoIter::as_slice` and `drain` all looked
verified. Adding the documented contract locally turned up 25 real obligations in
5 functions that had been passing without being checked. In ringbuf,
`<&mut [MaybeUninit<T>] as From<Ref<T>>>::from` was "SAFE" the same way.

The spec we used (the predicates are spelled out because `valid` / `aligned_to`
aren't exported from flux-core's `ptr` module):

```rust
#[flux_rs::extern_spec(core::slice)]
#[flux_rs::spec(fn(data: *const[@p] T, len: usize) -> &[T][len]
    requires p.addr != 0 && p.addr % T::align_of() == 0
          && p.addr >= p.base && p.size >= 0 && len * T::size_of() <= p.size)]
unsafe fn from_raw_parts<'a, T>(data: *const T, len: usize) -> &'a [T];

#[flux_rs::extern_spec(core::slice)]
#[flux_rs::spec(fn(data: *mut[@p] T, len: usize) -> &mut [T][len]
    requires p.addr != 0 && p.addr % T::align_of() == 0
          && p.addr >= p.base && p.size >= 0 && len * T::size_of() <= p.size)]
unsafe fn from_raw_parts_mut<'a, T>(data: *mut T, len: usize) -> &'a mut [T];
```

`alloc::alloc::{alloc, realloc, dealloc}` are unspecified too. So a `ptr::write`
into a fresh allocation can't be checked: it needs "null, or `layout.size()` bytes
aligned to `layout.align()`", and `alloc` itself requires `layout.size() > 0`.
#1546 tracks the `Allocator` trait methods, not these free functions.

**Hit in:** thin-vec (`src/flux_specs.rs`; `header_with_capacity` blocked on `alloc`),
ringbuf.

---

## F5. Comment on #1715 (ICE `projections.rs:401` with a higher-ranked `FnMut` bound)

**Found while Fluxing:** thin-vec

**Comment:**

Another trigger for the same panic, smaller than the original repro: it needs no
closure value and no `impl Trait`, just an `Iterator` impl whose type parameter has
a higher-ranked `FnMut` bound. It still crashes on `main` `2cacd2ea`:

```rust
extern crate flux_core;
pub struct E<F> { f: F }
impl<F> Iterator for E<F> where F: FnMut(&mut u8) -> bool {
    type Item = u8;
    fn next(&mut self) -> Option<u8> { None }
}
```

```
error: internal compiler error: crates/flux-infer/src/projections.rs:401:13: impossible case reached
  ... Normalizer::normalize_projection_ty
  ... FnSig::deeply_normalize
  ... fn_subtyping::check_fn_subtyping
  ... flux_refineck::checker::trait_impl_subtyping
```

`FnMut(&mut u8)` is `for<'a> FnMut(&'a mut u8)`, and `assemble_candidates_from_impls`
refuses the predicate with escaping bound vars while normalizing `<E as Iterator>::Item`
for the impl-vs-trait signature check. Without the reference (`F: FnMut(u8) -> bool`)
it falls through to #1821 instead. With a user trait in place of `Iterator` (no
flux-core spec), there's no crash. Found in thin-vec's
`impl<T, F> Iterator for ExtractIf<'_, T, F> where F: FnMut(&mut T) -> bool`.
