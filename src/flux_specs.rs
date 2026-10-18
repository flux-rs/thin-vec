//! CENTRIFUGE: flux-only extern specs. Nothing here affects compiled code.
//!
//! flux-core has no spec for `core::slice::from_raw_parts{,_mut}`, so without
//! these their safety precondition is never checked and the returned slice's
//! length is unknown. That would make every slice accessor (`as_slice`,
//! `as_mut_slice`, `Deref`, ...) look verified when it isn't. The precondition
//! is the documented one: `data` is non-null and aligned (even for `len == 0`),
//! and `len * size_of::<T>()` bytes from it lie inside one allocation.
//! (`valid` / `aligned_to` in flux-core's `ptr` module aren't exported, so the
//! predicates are spelled out.)

use core::slice;

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
