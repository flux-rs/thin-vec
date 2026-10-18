//! CENTRIFUGE: regression tests for bugs in thin-vec 0.2.21 (upstream 326a226)
//! found while Fluxing the crate. See `log/BUGS.md`.
#![forbid(unsafe_code)]

use std::ops::Bound::{Excluded, Unbounded};
use thin_vec::{ThinVec, thin_vec};

// Bug 1: `ThinVec::<ZST>::insert` writes the new length through `header_mut()`.
// Since #92 (0.2.20) a ZST `ThinVec` has no header: `ptr` holds `len + 1`. So
// this dereferences address `len + 1`. Release builds segfault (or worse); debug
// builds panic on `debug_assert!(!Self::is_zst())`. Miri (release):
//
//     cargo +nightly miri test --release --test flux_found_bugs
//
// reports "constructing invalid value of type &mut thin_vec::Header: encountered
// a dangling reference (0x1[noalloc] has no provenance)".

#[test]
#[cfg_attr(not(miri), ignore = "segfaults in release, panics in debug; run under Miri")]
fn zst_insert_into_empty() {
    let mut v: ThinVec<()> = ThinVec::new();
    v.insert(0, ());
    assert_eq!(v.len(), 1);
}

#[test]
#[cfg_attr(not(miri), ignore = "segfaults in release, panics in debug; run under Miri")]
fn zst_insert_middle() {
    let mut v: ThinVec<()> = thin_vec![(), (), ()];
    v.insert(1, ());
    assert_eq!(v.len(), 4);
}

// Bug 2 (logic, not UB): `drain` computes `n + 1` for `Excluded(n)` start and
// `Included(n)` end bounds without overflow checks. In release, `usize::MAX + 1`
// wraps to 0, so the call drains the wrong range instead of panicking like
// `Vec::drain`. Debug builds panic with "attempt to add with overflow", so these
// tests only demonstrate the bug in release: `cargo test --release`.

#[test]
#[cfg_attr(debug_assertions, ignore = "overflow panics in debug; the bug shows in release")]
fn drain_excluded_usize_max_start_drains_everything() {
    let mut v = thin_vec![1, 2, 3];
    let r = std::panic::catch_unwind(move || v.drain((Excluded(usize::MAX), Unbounded)).count());
    // Expected (std `Vec`): panic. Actual: Ok(3), every element drained.
    assert!(r.is_err(), "drained {:?} elements instead of panicking", r.ok());
}

#[test]
#[cfg_attr(debug_assertions, ignore = "overflow panics in debug; the bug shows in release")]
fn drain_inclusive_usize_max_end_drains_nothing() {
    let mut v = thin_vec![1, 2, 3];
    let r = std::panic::catch_unwind(move || v.drain(..=usize::MAX).count());
    // Expected (std `Vec`): panic. Actual: Ok(0), nothing drained.
    assert!(r.is_err(), "drained {:?} elements instead of panicking", r.ok());
}
