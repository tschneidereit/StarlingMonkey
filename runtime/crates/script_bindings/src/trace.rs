/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use js::jsapi::JSObject;
use js::jsapi::JSTracer;
use js::gc::Handle;
use js::jsapi::TraceKind;
use js::jsapi::GCTraceKindToAscii;
use js::jsapi::CallObjectTracer;
use js::jsapi::Heap;
use std::cell::OnceCell;
use std::fmt::Display;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

use js::gc::GCMethods;

use js::gc::Traceable as JSTraceable;
use crate::reflector::Reflector;

use log::trace;

/// Trace the `JSObject` held by `reflector`.
///
/// # Safety
/// tracer must point to a valid, non-null JS tracer.
#[cfg_attr(crown, allow(crown::unrooted_must_root))]
pub unsafe fn trace_reflector(tracer: *mut JSTracer, description: &str, reflector: &Reflector) {
    trace!("tracing reflector {}", description);
    unsafe { trace_object(tracer, description, reflector.rootable()) }
}

/// Trace a `JSObject`.
///
/// # Safety
/// tracer must point to a valid, non-null JS tracer.
pub(crate) unsafe fn trace_object(
    tracer: *mut JSTracer,
    description: &str,
    obj: &Heap<*mut JSObject>,
) {
    unsafe {
        trace!("tracing {}", description);
        CallObjectTracer(
            tracer,
            obj.ptr.get() as *mut _,
            GCTraceKindToAscii(TraceKind::Object),
        );
    }
}

// /// For use on non-jsmanaged types
// /// Use #[derive(JSTraceable)] on JS managed types
// macro_rules! unsafe_no_jsmanaged_fields(
//     ($($ty:ty),+) => (
//         $(
//             #[allow(unsafe_code)]
//             unsafe impl crate::JSTraceable for $ty {
//                 #[inline]
//                 unsafe fn trace(&self, _: *mut ::js::jsapi::JSTracer) {
//                     // Do nothing
//                 }
//             }
//         )+
//     );
// );

// unsafe_no_jsmanaged_fields!(DOMString);
// unsafe_no_jsmanaged_fields!(USVString);
// unsafe_no_jsmanaged_fields!(Error);

/// A trait to allow tracing only DOM sub-objects.
///
/// # Safety
///
/// This trait is unsafe; if it is implemented incorrectly, the GC may end up collecting objects
/// that are still reachable.
pub unsafe trait CustomTraceable {
    /// Trace `self`.
    ///
    /// # Safety
    ///
    /// The `JSTracer` argument must point to a valid `JSTracer` in memory. In addition,
    /// implementors of this method must ensure that all active objects are properly traced
    /// or else the garbage collector may end up collecting objects that are still reachable.
    unsafe fn trace(&self, trc: *mut JSTracer);
}

unsafe impl<T: CustomTraceable> CustomTraceable for Box<T> {
    #[inline]
    unsafe fn trace(&self, trc: *mut JSTracer) {
        unsafe { (**self).trace(trc) };
    }
}

unsafe impl<T: JSTraceable> CustomTraceable for OnceCell<T> {
    unsafe fn trace(&self, tracer: *mut JSTracer) {
        if let Some(value) = self.get() {
            unsafe { value.trace(tracer) }
        }
    }
}

/// Roots any JSTraceable thing
///
/// If you have a valid DomObject, use DomRoot.
/// If you have GC things like *mut JSObject or JSVal, use rooted!.
/// If you have an arbitrary number of DomObjects to root, use rooted_vec!.
/// If you know what you're doing, use this.
#[cfg_attr(crown, crown::unrooted_must_root_lint::allow_unrooted_interior)]
pub struct RootedTraceableBox<T: JSTraceable + 'static>(js::gc::RootedTraceableBox<T>);

unsafe impl<T: JSTraceable + 'static> JSTraceable for RootedTraceableBox<T> {
    unsafe fn trace(&self, tracer: *mut JSTracer) {
        unsafe { self.0.trace(tracer) };
    }
}

impl<T: JSTraceable + 'static> RootedTraceableBox<T> {
    /// DomRoot a JSTraceable thing for the life of this RootedTraceableBox
    pub fn new(traceable: T) -> RootedTraceableBox<T> {
        Self(js::gc::RootedTraceableBox::new(traceable))
    }

    /// Consumes a boxed JSTraceable and roots it for the life of this RootedTraceableBox.
    pub fn from_box(boxed_traceable: Box<T>) -> RootedTraceableBox<T> {
        Self(js::gc::RootedTraceableBox::from_box(boxed_traceable))
    }
}

impl<T> RootedTraceableBox<Heap<T>>
where
    Heap<T>: JSTraceable + 'static,
    T: GCMethods + Copy,
{
    pub fn handle(&self) -> Handle<T> {
        self.0.handle()
    }
}

// impl<T: JSTraceable + MallocSizeOf> MallocSizeOf for RootedTraceableBox<T> {
//     fn size_of(&self, ops: &mut MallocSizeOfOps) -> usize {
//         // Briefly resurrect the real Box value so we can rely on the existing calculations.
//         // Then immediately forget about it again to avoid dropping the box.
//         let inner = unsafe { Box::from_raw(self.0.ptr()) };
//         let size = inner.size_of(ops);
//         mem::forget(inner);
//         size
//     }
// }

impl<T: JSTraceable + Default> Default for RootedTraceableBox<T> {
    fn default() -> RootedTraceableBox<T> {
        RootedTraceableBox::new(T::default())
    }
}

impl<T: JSTraceable> Deref for RootedTraceableBox<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.0.deref()
    }
}

impl<T: JSTraceable> DerefMut for RootedTraceableBox<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.0.deref_mut()
    }
}

/// Wrapper type for nop traceble
///
/// SAFETY: Inner type must not impl JSTraceable
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(crown, crown::trace_in_no_trace_lint::must_not_have_traceable)]
pub(crate) struct NoTrace<T>(pub(crate) T);

impl<T: Display> Display for NoTrace<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T> From<T> for NoTrace<T> {
    fn from(item: T) -> Self {
        Self(item)
    }
}

#[allow(unsafe_code)]
unsafe impl<T> JSTraceable for NoTrace<T> {
    #[inline]
    unsafe fn trace(&self, _: *mut JSTracer) {}
}

// impl<T: MallocSizeOf> MallocSizeOf for NoTrace<T> {
//     fn size_of(&self, ops: &mut MallocSizeOfOps) -> usize {
//         self.0.size_of(ops)
//     }
// }
