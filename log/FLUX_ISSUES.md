# Flux problems found while Fluxing thin-vec

Driver: local `kris-gaudel/auto-strong-callgraph`, rebased onto upstream
`main` `2cacd2ea` (no newer upstream commits on 2026-10-18), flux libs
`2cacd2ea`, `nightly-2026-10-11`. The same driver ringbuf was Fluxed with;
`crates/ringbuf/log/FLUX_ISSUES.md` (branch `kris-gaudel/ringbuf`) has the
shared background. Upstream issues checked 2026-10-18, re-checked 2026-10-28. Ready-to-paste drafts of the
novel ones are in `TO_FILE.md`.

| # | kind | message / symptom | upstream | status |
|---|---|---|---|---|
| A | **ICE** (panic) | `builtin_assoc_refts.rs:135:76` `TooGeneric([T/#0; N/#1])` | **existing**: [#1800](https://github.com/flux-rs/flux/issues/1800) (fix PR #1801 open); likely fixed by #1874 (merged 2026-10-19, after this driver), unconfirmed | 1 fn `trusted` |
| B | **ICE** (internal error) | `infer.rs:937:22` ``incompatible types: `self: &'_ strg X[a0]` - `&'_ mut X` `` | **existing**: [#1821](https://github.com/flux-rs/flux/issues/1821) | 3 fns `trusted` + `trusted_impl` |
| C | **ICE** (panic) | `projections.rs:401:13: impossible case reached` (`assemble_candidates_from_impls`, trait predicate has escaping bound vars) | **existing**: [#1715](https://github.com/flux-rs/flux/issues/1715) (open); #1666 is the same site | 1 fn `trusted` + `trusted_impl` |
| 1 | incompleteness | no `T::size_of() >= 0` for generic `T` | **novel**, first found in ringbuf (ringbuf FLUX_ISSUES #1) | worked around in 5 fns; root of several ABORTs |
| 2 | name resolution | a **glob** import placed before the module it imports from doesn't resolve in flux annotations: `use m::*; mod m { pub type Sz = usize; }` → `#[field(Sz[n])]`: `cannot find type Sz in this scope` | **novel** | qualified path used (`impl_details::SizeType`) |
| 3 | incompleteness | a raw pointer's `addr` isn't known to be `>= 0`, so `ptr as usize - 1` on a `NonNull` (invariant `addr != 0`) can "underflow"; `ptr as usize` is typed `usize[addr]` without the usize range | **novel** on 2026-10-18; **fixed upstream by #1880** (merged 2026-10-21), unconfirmed on our driver: don't file | 1 ABORT (`ThinVec::len`, ZST branch) |
| 4 | missing flux-core specs | `core::slice::from_raw_parts{,_mut}` have no spec: their safety precondition is never checked and the slice length is unknown. Also no specs for the global `alloc::alloc::{alloc, realloc, dealloc}` | **novel**: not on the tracking list in [#1546](https://github.com/flux-rs/flux/issues/1546); #1761 is unrelated (the `slice as *const [T]` ICE); still missing on `main` 2026-10-28 | `from_raw_parts{,_mut}` specced locally (`src/flux_specs.rs`); `alloc` not (1 ABORT) |

## A. `TooGeneric` (existing: #1800)

`<[T; N] as TryFrom<ThinVec<T>>>::try_from` does
`ptr::read(vec.data_raw() as *const [T; N])`. `ptr::read`'s spec normalizes
`<[T; N]>::size_of()`, and `layout_of([T; N])` is `TooGeneric`. Same root
cause as ringbuf's trigger (an ADT or array *containing* a type parameter).
Confirmed to crash on its own (with its `trusted` removed). flux_thaw
attributed this one correctly.

## B. `Iterator::next` without a spec (existing: #1821)

`<IntoIter<T> as Iterator>::next`, `<Drain<'_, T> as Iterator>::next`,
`<Splice<'_, I> as Iterator>::next`. Same as ringbuf. `trusted` alone doesn't
silence it; `trusted_impl` on the impl block does.

## C. `projections.rs:401` with a higher-ranked `FnMut` bound (existing: #1715)

```
error: internal compiler error: crates/flux-infer/src/projections.rs:401:13: impossible case reached
  ... Normalizer::normalize_projection_ty
  ... FnSig::deeply_normalize
  ... fn_subtyping::check_fn_subtyping
  ... flux_refineck::checker::trait_impl_subtyping
```

Hit by `impl<T, F> Iterator for ExtractIf<'_, T, F> where F: FnMut(&mut T) ->
bool`. `FnMut(&mut T)` is `for<'a> FnMut(&'a mut T)`, and
`assemble_candidates_from_impls` refuses a predicate with escaping bound vars
while normalizing `<ExtractIf as Iterator>::Item` for the impl-vs-trait
signature check. Minimal repro (crashes on this driver; no closure value or
`impl Trait` needed, unlike #1715's):

```rust
extern crate flux_core;
pub struct E<F> { f: F }
impl<F> Iterator for E<F> where F: FnMut(&mut u8) -> bool {
    type Item = u8;
    fn next(&mut self) -> Option<u8> { None }
}
```

Without the reference (`F: FnMut(u8) -> bool`) it falls through to ICE B
instead. With a user trait instead of `Iterator` (no flux-core spec), there's
no crash. #1715's own repro crashes at the same line (`projections.rs:401:13`)
on this driver. Worth adding this shape to #1715 as a second trigger.

**Harness note:** the panic is raised before the def's `FLUX checking` line
is printed. So `flux_thaw.py` "fell back to the last DefId" and trusted the
previously checked function, 49 times, walking backwards through unrelated
impls (`borrow`, `as_ref`, `fmt`, `eq`, ...) until its iteration cap. Those 49
commits were reverted; the real site was found from the backtrace
(`trait_impl_subtyping`) and the def order.

## 1. No `T::size_of() >= 0` (novel; see ringbuf)

Here it blocks more than `ptr::read`. A pointer from a slice
(`<[T]>::as_mut_ptr`, size `n * T::size_of()`) only satisfies `in_bounds(p)`
(`p.size >= 0`) and `i * T::size_of() <= n * T::size_of()` for `i < n` when
`T::size_of() >= 0` is known. Experiment: adding `requires T::size_of() >= 0`
to the rewritten `remove` took it from 9 errors to 0. Applied to `remove`,
`swap_remove`, `dedup_by`, `dedup`, `dedup_by_key` (none has an in-crate
caller that can't carry it). Not applicable to `pop` (called from
`IntoIter::next_back`, a trait impl) or `Drain::next_back` (`ptr::read(&T)`).

## 2. Glob import before the module doesn't resolve (novel)

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

Works if the `mod` comes first, with a named import (`use m::Sz;`) in either
order, or with a path (`m::Sz`). It also fails on the driver from before
#1857 ("Resolve rust paths in flux definitions"), which was rebuilt as a side
sysroot to check, so it isn't a regression from that change. thin-vec has
exactly this shape: `use impl_details::*;` at the top, the `cfg`'d `mod
impl_details` below it.

## 3. Pointer addresses aren't known non-negative (novel)

```rust
extern crate flux_core;
use core::ptr::NonNull;
pub fn a(p: NonNull<u8>) -> usize { (p.as_ptr() as usize) - 1 }   // "may underflow"

#[flux_rs::spec(fn(NonNull<u8>{p: p.addr >= 0}) -> usize)]
pub fn b(p: NonNull<u8>) -> usize { (p.as_ptr() as usize) - 1 }   // ok

#[flux_rs::spec(fn(NonNull<u8>[@b, @a, @s]) -> usize{v: v == a && v >= 0})]
pub fn c(p: NonNull<u8>) -> usize { p.as_ptr() as usize }         // error: v >= 0
```

`NonNull`'s invariant gives `addr != 0`, and #1794 makes `p as usize` equal
`addr`, but nothing says `addr >= 0`. So the cast yields a `usize` whose
index may be negative (`c`). Suggested fix: `0 <= addr <= usize::MAX` as a
raw-pointer/`NonNull` invariant, or at least on the result of ptr-to-int
casts. Hit by `ThinVec::len`'s ZST branch, `(self.ptr.as_ptr() as usize) -
1`, which holds `len + 1` in a `NonNull`.

## 4. Unspecified `slice::from_raw_parts{,_mut}` and global allocator fns (novel)

Without a spec, `slice::from_raw_parts(p, n)` is checked against nothing, and
flux knows nothing about the result's length. In thin-vec that made
`as_slice`, `as_mut_slice`, `Deref`/`DerefMut`, `IntoIter::as_slice` and
`drain`'s iterator look verified. `src/flux_specs.rs` adds the documented
contract (non-null, aligned even for `len == 0`, `len * size_of` bytes in one
allocation; result `[T][len]`). That added 25 real obligations in 5
functions that had passed vacuously, and it's what let `remove`/`swap_remove`/`dedup_by` verify
(the slice gives a sized pointer). ringbuf's `<&mut [MaybeUninit<T>] as
From<Ref<T>>>::from` (`slice::from_raw_parts_mut`) was SAFE vacuously for the
same reason.

`alloc::alloc::{alloc, realloc, dealloc}` are also unspecified, so
`header_with_capacity`'s `ptr::write` into a fresh allocation can't be checked
(it'd need "null, or `layout.size()` bytes aligned to `layout.align()`", and
`alloc` itself requires `layout.size() > 0`). #1546 tracks the `Allocator`
trait methods, not these.
