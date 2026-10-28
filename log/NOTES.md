# Harness notes — thin-vec

Target: `mozilla/thin-vec` main `326a226` (0.2.21), vendored with
`git subtree add --prefix=crates/thin-vec ... main --squash`. One file,
`src/lib.rs` (~3.2 kLOC without tests), default features (`std`; not
`gecko-ffi`, `unstable`, `serde`, `malloc_size_of`).

See also: `ABORT.md` (every remaining error), `FLUX_ISSUES.md` (ICEs and flux
problems, existing vs novel), `BUGS.md` (thin-vec bugs).

## Score (2026-10-28)

`cargo flux` is clean: **199 functions processed: 171 checked; 29 trusted**.

| trusted reason | functions |
|---|---:|
| `ICE:` | 5 (+ 5 `trusted_impl` on their impls) |
| `ABORT:` | 23 |
| `BUG:` | 1 (`insert`, BUGS.md bug 1) |

The 24 ABORT/BUG functions are exactly the ones that held the 100 errors below;
each `reason` cites its `ABORT.md` section. `drain` (BUGS.md bug 2) is
`ABORT:`, not `BUG:`: flux doesn't flag its `+ 1` overflow, and its 7 errors are
the slice-accessor obligation.

## Result (2026-10-18)

| | |
|---|---|
| refinement errors | baseline 108 (after ICEs trusted) → **100**, all in `ABORT.md`. The count includes 25 newly checked `from_raw_parts` obligations that were vacuous before; on the same footing it's 108 + 25 = 133 → 100 |
| ICEs | **3 distinct, all existing upstream**: A #1800 (1 fn), B #1821 (3 fns), C #1715 (1 fn) |
| novel flux problems | 4 (no crashes): generic `size_of >= 0` (also in ringbuf); glob-import-before-module resolution; pointer `addr` not known `>= 0`; no flux-core specs for `slice::from_raw_parts{,_mut}` / global `alloc` |
| thin-vec bugs | **1 novel UB bug** (ZST `insert`, segfault from safe code, regression from 0.2.20) + 1 logic bug (`drain` bound overflow). `BUGS.md`, `tests/flux_found_bugs.rs` |
| tests | 87 unit + 50 doc pass; unit tests clean under Miri |

## Setup

As for ringbuf (same driver, `2cacd2ea`; no newer upstream commits today):
flux git deps + `[package.metadata.flux]` in `Cargo.toml` (and `cfg(flux)`
added to the existing `check-cfg`), libs pinned with `cargo update -p
flux-rs --precise 2cacd2ea00` (`Cargo.lock` is gitignored), toolchain
`nightly-2026-10-11`, and in `src/lib.rs` `extern crate flux_core;` /
`flux_alloc`, `cfg_attr(flux, allow(unused_features))`, `#[cfg(flux)] mod
flux_specs;`.

## ICE handling

`flux_thaw.py` was fixed first (`harness:` commit `49054d6`): it used to
`git init` inside any crate dir without its own `.git`, which nested a repo in
`crates/ringbuf`. It then handled ICE A correctly (iteration 1). For ICE C it
couldn't find the crashing def (the panic comes before that def's `FLUX
checking` line) and trusted 49 unrelated functions until its iteration cap.
Those commits were reset away and C was located by hand (backtrace →
`trait_impl_subtyping`, def order → `ExtractIf`'s `Iterator::next`). ICE B
(an error, not a panic) was trusted by hand, as in ringbuf.

## Phase 1 (types): 8 types

| uid | type | outcome |
|---|---|---|
| 0 | `Header` | DONE: `refined_by(len, cap)`, `invariant(len <= cap)`, exact specs on `len`/`cap`/`set_len`/`set_cap_and_auto` and the non-gecko `assert_size`/`pack_capacity_and_auto`/`unpack_capacity`. Constructors (`EMPTY_HEADER`, `header_with_capacity`) satisfy it; the two maintaining callers (`set_header_len`, `reallocate`) are protocol ABORTs. Field types are written `impl_details::SizeType`: the bare name hits FLUX_ISSUES #2 |
| 1 | `ThinVec` | no annotation: its state (`len`, `cap`, allocation size) is behind the `NonNull<Header>` (ABORT.md) |
| 2 | `ThinVec::clear::DropGuard` | no annotation (holds `&mut ThinVec`) |
| 3 | `IntoIter` | no annotation: `start <= vec.len()` relates a field to heap memory |
| 4 | `Drain` | no annotation: `end`/`tail` relate to the vec's heap `len` |
| 5 | `ExtractIf` | DONE: `refined_by(idx, end, del, old_len)`, `invariant(del <= idx && idx <= end && end <= old_len)`; `extract_if::slice_range` gets `-> Range{r: r.start <= r.end && r.end <= len}`. Discharges `size_hint`'s `end - idx` and `drop`'s `old_len - del` / `old_len - idx` |
| 6 | `IntoIter::drop::drop_non_singleton::DropGuard` | no annotation |
| 7 | `Splice` | no annotation (wraps `Drain`) |

## Phase 2 (functions)

The full sweep checks 195 defs plus the 5 trusted ones (`log/targets.json`
has 238 entries, some duplicated). Driven from the sweep's error list, as for
ringbuf.

| what | result |
|---|---|
| `ExtractIf::{size_hint, drop}`, `extract_if`, `slice_range` | DONE via the type 5 invariant (cursor arithmetic; `drop`'s pointer ops stay in ABORT §1) |
| `split_off` (uid 214) | underflow DONE: `assert!(at <= old_len)` moved before `old_len - at` (was after; benign, but debug builds panicked with the wrong message). Copy into spare capacity stays ABORT |
| `retain_mut` (+ `retain`) | DONE: `len` taken from the slice it indexes |
| `remove`, `swap_remove`, `dedup_by` (+ `dedup`, `dedup_by_key`) | DONE: `(ptr, len)` from one `as_mut_slice()` + `requires T::size_of() >= 0` (FLUX_ISSUES #1) |
| `pop` | rewritten the same way; ABORT (its trait-impl caller can't carry the precondition) |
| `src/flux_specs.rs` | `slice::from_raw_parts{,_mut}` extern specs (FLUX_ISSUES #4) |
| 24 functions, 100 errors | ABORT, see ABORT.md |
| 5 functions | `trusted` (ICE) |
| the rest (107 rows) | SAFE |

All rewrites were rechecked: `cargo test` passes, `cargo +nightly-2026-08-21
miri test --lib` passes (87 tests), and the ZST and panic-safety Miri sweeps
used for BUGS.md were rerun against the modified crate for the touched
functions.

## Source changes (all marked `CENTRIFUGE`)

| where | change | why |
|---|---|---|
| `Cargo.toml`, `rust-toolchain.toml`, `src/lib.rs` header | flux setup | above |
| `src/flux_specs.rs` (new, `cfg(flux)`) | `from_raw_parts{,_mut}` specs | FLUX_ISSUES #4 |
| `Header`, `impl_details` (non-gecko) | invariant + specs | Phase 1 |
| `ExtractIf`, `slice_range` | invariant + spec | Phase 1 |
| `split_off` | assert before subtraction | above |
| `retain_mut` | `len` from the slice | above |
| `pop`, `remove`, `swap_remove`, `dedup_by` | `(ptr, len)` from `as_mut_slice()` | above |
| `remove`, `swap_remove`, `dedup`, `dedup_by`, `dedup_by_key` | `requires T::size_of() >= 0` | FLUX_ISSUES #1 |
| 5 fns / 5 impls | `trusted` / `trusted_impl` citing #1800 / #1821 / #1715 | ICEs |
| `tests/flux_found_bugs.rs` (new) | reproducers | BUGS.md |

## Per-def status (final sweep)

One row per source span. A uid of "—" means the def path wasn't matched in
the call-graph dump (impl methods are named `{impl#N}` there).

136 rows: ABORT 24, SAFE 107, TRUSTED 5; 100 errors; unowned 0

| uid | def | span | status |
|---|---|---|---|
| 9 | `impl_details::assert_size` | src/lib.rs:193 | SAFE |
| 14 | `impl_details::pack_capacity_and_auto` | src/lib.rs:199 | SAFE |
| 0 | `impl_details::unpack_capacity` | src/lib.rs:206 | SAFE |
| 16 | `impl_details::is_auto` | src/lib.rs:211 | SAFE |
| 15 | `capacity_overflow` | src/lib.rs:295 | SAFE |
| — | `{impl#0}::unwrap_cap_overflow` | src/lib.rs:304 | SAFE |
| — | `{impl#1}::unwrap_cap_overflow` | src/lib.rs:313 | SAFE |
| — | `{impl#2}::len` | src/lib.rs:352 | SAFE |
| — | `{impl#2}::set_len` | src/lib.rs:358 | SAFE |
| — | `{impl#2}::cap` | src/lib.rs:363 | SAFE |
| — | `{impl#2}::set_cap_and_auto` | src/lib.rs:368 | SAFE |
| — | `{impl#2}::is_auto` | src/lib.rs:378 | SAFE |
| — | `EMPTY_HEADER` | src/lib.rs:389 | SAFE |
| 77 | `alloc_size` | src/lib.rs:404 | SAFE |
| 48 | `padding` | src/lib.rs:431 | SAFE |
| 13 | `alloc_align` | src/lib.rs:448 | SAFE |
| 92 | `layout` | src/lib.rs:460 | SAFE |
| 105 | `header_with_capacity` | src/lib.rs:471 | ABORT (2) |
| 34 | `len_to_ptr_unchecked` | src/lib.rs:503 | SAFE |
| — | `{impl#5}::is_zst` | src/lib.rs:560 | SAFE |
| — | `{impl#5}::new` | src/lib.rs:567 | SAFE |
| — | `{impl#5}::with_capacity` | src/lib.rs:640 | SAFE |
| — | `{impl#5}::ptr` | src/lib.rs:673 | SAFE |
| — | `{impl#5}::header` | src/lib.rs:681 | SAFE |
| — | `{impl#5}::data_raw` | src/lib.rs:686 | ABORT (2) |
| — | `{impl#5}::header_mut` | src/lib.rs:737 | SAFE |
| — | `{impl#5}::len` | src/lib.rs:754 | ABORT (1) |
| — | `{impl#5}::is_empty` | src/lib.rs:775 | SAFE |
| — | `{impl#5}::capacity` | src/lib.rs:790 | SAFE |
| — | `{impl#5}::has_capacity` | src/lib.rs:799 | SAFE |
| — | `{impl#5}::set_len` | src/lib.rs:884 | SAFE |
| — | `{impl#5}::set_len_zst` | src/lib.rs:899 | SAFE |
| — | `{impl#5}::set_header_len` | src/lib.rs:912 | ABORT (1) |
| — | `{impl#5}::set_len_non_singleton` | src/lib.rs:920 | SAFE |
| — | `{impl#5}::push` | src/lib.rs:946 | SAFE |
| — | `{impl#5}::push_unchecked` | src/lib.rs:965 | ABORT (4) |
| — | `{impl#5}::pop` | src/lib.rs:987 | ABORT (3) |
| — | `{impl#5}::insert` | src/lib.rs:1021 | ABORT (12) |
| — | `{impl#5}::remove` | src/lib.rs:1062 | SAFE |
| — | `{impl#5}::swap_remove` | src/lib.rs:1107 | SAFE |
| — | `{impl#5}::truncate` | src/lib.rs:1170 | ABORT (3) |
| — | `{impl#5}::clear` | src/lib.rs:1198 | SAFE |
| — | `{impl#5}::clear::{impl#0}::drop` | src/lib.rs:1203 | SAFE |
| — | `{impl#5}::as_slice` | src/lib.rs:1227 | ABORT (5) |
| — | `{impl#5}::as_mut_slice` | src/lib.rs:1243 | ABORT (5) |
| — | `{impl#5}::reserve` | src/lib.rs:1255 | SAFE |
| — | `{impl#5}::reserve_exact` | src/lib.rs:1341 | SAFE |
| — | `{impl#5}::shrink_to_fit` | src/lib.rs:1371 | SAFE |
| — | `{impl#5}::retain` | src/lib.rs:1425 | SAFE |
| — | `{impl#5}::retain_mut` | src/lib.rs:1453 | SAFE |
| — | `{impl#5}::dedup_by_key` | src/lib.rs:1497 | SAFE |
| — | `{impl#5}::dedup_by` | src/lib.rs:1531 | SAFE |
| — | `{impl#5}::split_off` | src/lib.rs:1587 | ABORT (7) |
| — | `{impl#5}::append` | src/lib.rs:1623 | SAFE |
| — | `{impl#5}::drain` | src/lib.rs:1659 | ABORT (7) |
| — | `{impl#5}::splice` | src/lib.rs:1729 | SAFE |
| — | `{impl#5}::extract_if` | src/lib.rs:1790 | SAFE |
| — | `{impl#5}::extract_if::slice_index_fail` | src/lib.rs:1799 | SAFE |
| — | `{impl#5}::extract_if::slice_range` | src/lib.rs:1828 | SAFE |
| — | `{impl#5}::reallocate` | src/lib.rs:1881 | ABORT (1) |
| — | `{impl#5}::is_singleton` | src/lib.rs:1931 | SAFE |
| — | `{impl#5}::is_auto_array` | src/lib.rs:1958 | SAFE |
| — | `{impl#5}::uses_stack_allocated_buffer` | src/lib.rs:1963 | SAFE |
| — | `{impl#5}::has_allocation` | src/lib.rs:1971 | SAFE |
| — | `{impl#6}::resize` | src/lib.rs:1999 | SAFE |
| — | `{impl#6}::extend_from_slice` | src/lib.rs:2038 | SAFE |
| — | `{impl#7}::dedup` | src/lib.rs:2065 | SAFE |
| 206 | `drop_non_singleton` | src/lib.rs:2072 | SAFE |
| 219 | `drop_thin_vec` | src/lib.rs:2089 | SAFE |
| — | `{impl#8}::drop` | src/lib.rs:2102 | SAFE |
| — | `{impl#9}::deref` | src/lib.rs:2122 | SAFE |
| — | `{impl#10}::deref_mut` | src/lib.rs:2128 | SAFE |
| — | `{impl#11}::index` | src/lib.rs:2136 | ABORT (1) |
| — | `{impl#12}::index_mut` | src/lib.rs:2142 | ABORT (2) |
| — | `{impl#13}::borrow` | src/lib.rs:2148 | SAFE |
| — | `{impl#14}::borrow_mut` | src/lib.rs:2154 | SAFE |
| — | `{impl#15}::as_ref` | src/lib.rs:2160 | SAFE |
| — | `{impl#16}::extend` | src/lib.rs:2167 | SAFE |
| — | `{impl#17}::fmt` | src/lib.rs:2193 | SAFE |
| — | `{impl#18}::hash` | src/lib.rs:2202 | SAFE |
| — | `{impl#19}::partial_cmp` | src/lib.rs:2215 | SAFE |
| — | `{impl#20}::cmp` | src/lib.rs:2225 | SAFE |
| — | `{impl#21}::eq` | src/lib.rs:2235 | SAFE |
| — | `{impl#22}::eq` | src/lib.rs:2245 | SAFE |
| — | `{impl#23}::eq` | src/lib.rs:2255 | SAFE |
| — | `{impl#24}::eq` | src/lib.rs:2265 | SAFE |
| — | `{impl#61}::eq` | src/lib.rs:2348 | SAFE |
| — | `{impl#62}::eq` | src/lib.rs:2353 | SAFE |
| — | `{impl#26}::into_iter` | src/lib.rs:2371 | SAFE |
| — | `{impl#27}::into_iter` | src/lib.rs:2383 | SAFE |
| — | `{impl#28}::into_iter` | src/lib.rs:2392 | SAFE |
| — | `{impl#29}::clone` | src/lib.rs:2402 | SAFE |
| — | `{impl#29}::clone::clone_non_singleton` | src/lib.rs:2405 | ABORT (4) |
| — | `{impl#30}::default` | src/lib.rs:2432 | SAFE |
| — | `{impl#31}::from_iter` | src/lib.rs:2439 | SAFE |
| — | `{impl#32}::from` | src/lib.rs:2457 | SAFE |
| — | `{impl#33}::from` | src/lib.rs:2472 | SAFE |
| — | `{impl#34}::from` | src/lib.rs:2487 | SAFE |
| — | `{impl#35}::from` | src/lib.rs:2506 | SAFE |
| — | `{impl#36}::from` | src/lib.rs:2525 | SAFE |
| — | `{impl#37}::from` | src/lib.rs:2543 | SAFE |
| — | `{impl#38}::from` | src/lib.rs:2562 | SAFE |
| — | `{impl#39}::from` | src/lib.rs:2577 | SAFE |
| — | `{impl#40}::as_slice` | src/lib.rs:2672 | ABORT (7) |
| — | `{impl#40}::as_mut_slice` | src/lib.rs:2691 | SAFE |
| — | `{impl#40}::as_raw_mut_slice` | src/lib.rs:2695 | ABORT (2) |
| — | `{impl#128}::size_hint` | src/lib.rs:2716 | ABORT (1) |
| — | `{impl#41}::next_back` | src/lib.rs:2723 | SAFE |
| — | `{impl#44}::drop` | src/lib.rs:2742 | SAFE |
| — | `{impl#44}::drop::drop_non_singleton` | src/lib.rs:2745 | SAFE |
| — | `{impl#44}::drop::drop_non_singleton::{impl#0}::drop` | src/lib.rs:2749 | SAFE |
| — | `{impl#45}::fmt` | src/lib.rs:2768 | SAFE |
| — | `{impl#46}::as_ref` | src/lib.rs:2774 | SAFE |
| — | `{impl#47}::clone` | src/lib.rs:2781 | SAFE |
| — | `{impl#129}::size_hint` | src/lib.rs:2892 | SAFE |
| — | `{impl#48}::next_back` | src/lib.rs:2898 | ABORT (1) |
| — | `{impl#51}::drop` | src/lib.rs:2912 | ABORT (8) |
| — | `{impl#52}::fmt` | src/lib.rs:2933 | SAFE |
| — | `{impl#53}::as_slice` | src/lib.rs:2953 | SAFE |
| — | `{impl#54}::as_ref` | src/lib.rs:2961 | SAFE |
| — | `{impl#130}::fmt` | src/lib.rs:2980 | SAFE |
| — | `{impl#131}::size_hint` | src/lib.rs:2995 | SAFE |
| — | `{impl#55}::next_back` | src/lib.rs:3001 | SAFE |
| — | `{impl#57}::drop` | src/lib.rs:3009 | SAFE |
| — | `{impl#58}::fill` | src/lib.rs:3154 | ABORT (9) |
| — | `{impl#58}::move_tail` | src/lib.rs:3175 | ABORT (8) |
| — | `{impl#132}::size_hint` | src/lib.rs:3247 | SAFE |
| — | `{impl#59}::drop` | src/lib.rs:3253 | ABORT (4) |
| — | `{impl#60}::write` | src/lib.rs:3280 | SAFE |
| — | `{impl#60}::write_all` | src/lib.rs:3286 | SAFE |
| — | `{impl#60}::flush` | src/lib.rs:3292 | SAFE |
| — | `impl<T, const N: usize> TryFrom<ThinVec<T>> for [T; N]` :: `try_from` | src/lib.rs:2622 | TRUSTED (ICE, #1800) |
| — | `impl<T> Iterator for IntoIter<T>` :: `next` | src/lib.rs:2704 | TRUSTED (ICE, #1821) |
| — | `impl<'a, T> Iterator for Drain<'a, T>` :: `next` | src/lib.rs:2888 | TRUSTED (ICE, #1821) |
| — | `impl<I: Iterator> Iterator for Splice<'_, I>` :: `next` | src/lib.rs:2991 | TRUSTED (ICE, #1821) |
| — | `impl<T, F> Iterator for ExtractIf<'_, T, F>` :: `next` | src/lib.rs:3223 | TRUSTED (ICE, #1715) |
