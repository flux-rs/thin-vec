# Bugs found in thin-vec while Fluxing

Target: thin-vec 0.2.21, upstream `mozilla/thin-vec` main `326a226`
(vendored as `crates/thin-vec`). Reproducers: `tests/flux_found_bugs.rs`.

Checked against upstream issues and RustSec on 2026-10-18. The one thin-vec
advisory, RUSTSEC-2026-0103 (`IntoIter::drop` / `clear` panic double free, fixed
in 0.2.16), is unrelated, and no issue covers either bug. Both are **novel**.
**Not reported upstream yet.**

## Bug 1: `ThinVec::<ZST>::insert` dereferences the fake length pointer (UB from safe code)

**Severity: memory safety. Safe code, any zero-sized `T`, release builds
segfault. Regression from #92 "Remove allocation for ZSTs" (0.2.20).**

Since #92, a ZST `ThinVec` has no allocation or header. `self.ptr` stores
`len + 1` as the address (`len_to_ptr_unchecked`), and `len()` / `set_len_zst`
decode it. Every length update goes through `set_len_non_singleton`, which
dispatches to `set_len_zst` for ZSTs. **Except `insert`**, which still ends
with:

```rust
self.set_header_len(old_len + 1);   // -> header_mut() -> &mut *self.ptr()
```

For a ZST that forms `&mut Header` at address `len + 1` and writes to it:

```rust
#![forbid(unsafe_code)]
let mut v: thin_vec::ThinVec<()> = thin_vec::ThinVec::new();
v.insert(0, ());
```

| build | result |
|---|---|
| release | **SIGSEGV** (exit 139) |
| debug | panic: `assertion failed: !Self::is_zst()` (the `debug_assert!` in `header_mut`) |
| Miri `--release` | `Undefined Behavior: constructing invalid value of type &mut thin_vec::Header: encountered a dangling reference (0x1[noalloc] has no provenance)` (`0x4` after three pushes) |

A real address can be hit too: for a ZST vec of length `n`, the write goes to
`n + 1`, so it isn't limited to the null page.

Every other public method was also run on a ZST `ThinVec` under Miri (release)
and as a native debug build: `push`/`pop`, `remove`, `swap_remove`,
`truncate`, `clear`, slices, `reserve*`/`shrink_to_fit`, `retain{,_mut}`,
`dedup`, `split_off`, `append`, `drain` (both ends), `splice`, `extract_if`,
`resize`, `extend*`, `clone`, `into_iter` (both ends), `Vec` conversions,
`TryFrom` for arrays, `FromIterator`, `thin_vec!`, `with_capacity`, indexing,
`Eq`/`Debug`. Only `insert` is affected.

Fix: replace `self.set_header_len(old_len + 1)` with
`self.set_len_non_singleton(old_len + 1)`, as `push_unchecked` does. Upstream
`src/lib.rs` L1005 (L1014 here).

Tests: `zst_insert_into_empty`, `zst_insert_middle` (ignored outside Miri):

    cargo +nightly-2026-08-21 miri test --release --test flux_found_bugs -- --exact zst_insert_into_empty

**How Flux led here:** Flux flags `insert` with 12 errors, all on raw-pointer
offsets from `data_raw()` that rely on `len`/`cap` invariants. Comparing
`insert` against `push_unchecked` (which Flux flags the same way) showed that
`insert` writes the new length differently.

## Bug 2: `drain` range bounds overflow and drain the wrong range (logic, release only)

**Severity: wrong behavior, no UB.**

```rust
let start = match range.start_bound() { Bound::Excluded(&n) => n + 1, .. };
let end   = match range.end_bound()   { Bound::Included(&n) => n + 1, .. };
```

Neither `+ 1` is checked. `Vec::drain` (and thin-vec's own `extract_if`)
panic for these ranges. In release, thin-vec wraps instead:

| call on `thin_vec![1, 2, 3]` | `Vec` | `ThinVec` (release) |
|---|---|---|
| `drain((Excluded(usize::MAX), Unbounded))` | panics | drains **all** elements `[1, 2, 3]` |
| `drain(..=usize::MAX)` | panics | drains **nothing** |

Debug builds panic with `attempt to add with overflow`. Fix: `n.checked_add(1)`
with the same panics as `extract_if` (`slice_start_index_overflow_fail` /
`slice_end_index_overflow_fail`). Tests:
`drain_excluded_usize_max_start_drains_everything`,
`drain_inclusive_usize_max_end_drains_nothing` (they fail under
`cargo test --release`).

## Panic-safety sweep (no UB found)

23 scenarios with a panicking predicate, iterator, `Clone` or element `Drop`
inside `retain`, `retain_mut`, `dedup{,_by}`, `truncate`, `clear`, `Drop`,
`drain` (full and partial), `splice` (iterator and drop), `extract_if`
(predicate and drop), `into_iter`, `clone`, `resize`, `extend`,
`extend_from_slice`, `FromIterator`. Each was run under Miri with
`catch_unwind`: no UB, no double drop. Two leak elements, which is not UB but
does differ from `std::Vec`:

- `Drain::drop`: if an element's `Drop` panics while the rest of the range is
  being exhausted, the tail is never moved back (no guard), so the tail
  elements leak (`len` stays at the drain start: leak amplification).
- `Clone for ThinVec` (`clone_non_singleton`): a panicking `T::clone` leaks
  the elements already cloned into the new allocation.
