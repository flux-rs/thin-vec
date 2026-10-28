# ABORT log — thin-vec

Every refinement error left in the full sweep (**100 errors in 24 functions,
0 ICEs**). Each of the 24 functions is now `#[flux_rs::trusted(reason = "ABORT:
… (ABORT.md §n)")]` (`insert`: `"BUG: …"`), so `cargo flux` is clean. Each entry lists the strategies tried, in harness order: 1
precondition on this function, 2 `refined_by` on an upstream struct, 3
intermediate callee spec. Uids are from `log/targets.json` (the call-graph dump
lists some defs twice; the first uid is given). Line numbers are this
checkout's.

## The blocker: `len` and `cap` live behind the header pointer

A `ThinVec<T>` is one `NonNull<Header>`. `len` and `cap` are fields of the
heap `Header` it points to (or of the static `EMPTY_HEADER`; for ZSTs, `len + 1`
is the pointer itself). Flux doesn't track the contents of memory behind a raw
pointer. So:

- every `self.len()` / `self.capacity()` is a fresh `self.ptr.as_ref()` read,
  unrelated to the previous one (`Header` now has `invariant(len <= cap)`, but
  that only relates the two fields of *one* read);
- `data_raw()`, the element pointer `header + size_of::<Header>() + padding`,
  carries no allocation size. `ThinVec` could only be refined by its `ptr`'s
  `(base, addr, size)`, and an invariant tying `size` to `cap` would mention
  memory contents, which flux can't express (and `opaque` is off-limits).

**What does verify** is anything whose pointer comes from a slice of known
length. `slice::from_raw_parts{,_mut}` (specced in `src/flux_specs.rs`, see
FLUX_ISSUES #4) gives `[T][len]`, and `<[T]>::as_mut_ptr` gives a pointer of
`len * size_of` bytes. `remove`, `swap_remove` and `dedup_by` were rewritten
to take `(ptr, len)` from one `as_mut_slice()` (CENTRIFUGE, same operations)
and verify completely with `requires T::size_of() >= 0` (FLUX_ISSUES #1). The
remaining assumption is then concentrated in the slice accessors (§2).

## 1. Writes into spare capacity / reads past `len()`: needs `size == alloc_size(cap)`

Each needs "the allocation behind `ptr` has room for `cap` elements" plus a
`len`/`cap` fact from separate header reads.

| function | uid | errors | site |
|---|---|---:|---|
| `ThinVec::insert` | 224 | 12 | `ptr::copy(ptr.add(idx), ptr.add(idx + 1), old_len - idx)`, `ptr::write(ptr.add(idx), elem)` past `len`. (Also BUGS.md bug 1, the ZST case) |
| `ThinVec::push_unchecked` | 107 | 4 | `ptr::write(self.data_raw().add(old_len), val)` |
| `ThinVec::split_off` | 214 | 7 | `copy_nonoverlapping(self.data_raw().add(at), new_vec.data_raw(), new_vec_len)` into the new vec's spare capacity; `nonoverlapping` across two allocations |
| `Clone::clone::clone_non_singleton` | 213 | 4 | `ptr::write(data_raw, x.clone()); data_raw = data_raw.add(1)` into a fresh `with_capacity(len)` |
| `<Drain as Drop>::drop` | 108 | 8 | `ptr::copy(data_raw().add(self.end), data_raw().add(old_len), self.tail)`: the tail beyond the current `len` |
| `Drain::fill` | 126 | 9 | `from_raw_parts_mut(data_raw().add(len), self.end - len)` over the drained hole; `self.end - len` underflow (separate reads) |
| `Drain::move_tail` | 222 | 8 | `ptr::copy` of the tail into newly reserved space |
| `<ExtractIf as Drop>::drop` | 125 | 4 | `ptr.add(self.idx)`, `src.sub(self.del)` (the cursor arithmetic itself is verified by the new `ExtractIf` invariant; these are the pointer bounds) |

- 1: a `requires` would need the header's `cap` and the allocation size,
  neither of which is nameable.
- 2: tried. `Header` got `refined_by(len, cap)` + `invariant(len <= cap)`
  (DONE), but `ThinVec` can't link its `ptr`'s size to the pointee's `cap`.
- 3: the slice rewrite (above) doesn't apply. These touch memory *outside*
  `[0, len)`.

## 2. Slice accessors: `data_raw()` valid for `len` elements

| function | uid | errors |
|---|---|---:|
| `ThinVec::as_slice` | 85 | 5 |
| `ThinVec::as_mut_slice` | 87 | 5 |
| `IntoIter::as_slice` | 86 | 7 (`data_raw().add(self.start)`, `self.len()`) |
| `IntoIter::as_raw_mut_slice` | 88 | 2 |
| `ThinVec::drain` | 128 | 7 (`from_raw_parts(self.data_raw().add(start), end - start)`) |
| `ThinVec::data_raw` | 68 | 2 (`ptr.add(header_size + padding)`) |

These are the `from_raw_parts` obligations (non-null, aligned, `len * size_of
<= p.size`) that were never checked before FLUX_ISSUES #4. They're the root
assumption of every slice-based verification in the crate: one per accessor,
instead of one per pointer operation. Strategies as in §1. `data_raw` also
needs the singleton case: `EMPTY_HEADER` is 16 bytes, and the
`!empty_header_is_aligned && cap == 0` → `dangling()` branch depends on a
header read.

## 3. Repeated header reads (underflow / bounds)

| function | uid | errors | site |
|---|---|---:|---|
| `ThinVec::truncate` | 111 | 3 | `while len < self.len() { let new_len = self.len() - 1; .. data_raw().add(new_len) }`: two reads per iteration |
| `ThinVec::pop` | 110 | 3 | rewritten slice-based like `remove`; needs `T::size_of() >= 0`, but its caller `<IntoIter as DoubleEndedIterator>::next_back` is a trait impl, so the precondition can't be carried |
| `<IntoIter as Iterator>::size_hint` | 80 | 1 | `self.vec.len() - self.start` |
| `ThinVec::set_header_len` | 82 | 1 | new: `Header::set_len` requires `len <= cap` of a fresh header read |
| `ThinVec::reallocate` | 120 | 1 | new: `set_cap_and_auto(new_cap)` requires the realloc'ed header's `len <= new_cap` |

`set_header_len` / `reallocate` are where the new `Header` invariant is
*maintained* by the caller protocol. By inspection every caller satisfies it:
`reserve*` passes `new_cap >= len`, `shrink_to_fit` passes `len`, and
`push`/`insert` reserve first. A `truncate` rewrite like `pop`'s was
considered: it would need `requires T::size_of() >= 0` on `truncate` and so
on its callers `resize` and `retain_mut` (`dedup_by` already has it), and on
theirs (`retain`, `thin_vec![x; n]` via `resize`). Rejected as too much
spec churn for 3 errors.

## 4. Other

| function | uid | errors | cause |
|---|---|---:|---|
| `ThinVec::len` | 63 | 1 | ZST branch `(self.ptr.as_ptr() as usize) - 1`: FLUX_ISSUES #3 (pointer `addr` not known `>= 0`; `NonNull` only gives `!= 0`) |
| `header_with_capacity` | 104 | 2 | `ptr::write(alloc(layout) as *mut Header, ..)`: no spec for `alloc::alloc::alloc` (FLUX_ISSUES #4), and `alloc_size`'s `isize` arithmetic needs `T::size_of() >= 0` (#1) |
| `<Drain as DoubleEndedIterator>::next_back` | 19 | 1 | `ptr::read(x)` with `x: &T` from the slice iterator: FLUX_ISSUES #1, in a trait impl (no precondition possible) |
| `<ThinVec as Index<I>>::index` | 114 | 1 | `&self.deref()[index]` with a generic `I: SliceIndex<[T]>`: the bounds check (a panic) can't be discharged for an arbitrary index type |
| `<ThinVec as IndexMut<I>>::index_mut` | 117 | 2 | same |

## `trusted` (ICE, not ABORT)

| function | ICE |
|---|---|
| `<[T; N] as TryFrom<ThinVec<T>>>::try_from` | A, flux-rs/flux#1800 |
| `<IntoIter<T> as Iterator>::next` | B, #1821 |
| `<Drain<'_, T> as Iterator>::next` | B, #1821 |
| `<Splice<'_, I> as Iterator>::next` | B, #1821 |
| `<ExtractIf<'_, T, F> as Iterator>::next` | C, flux-rs/flux#1715 |
