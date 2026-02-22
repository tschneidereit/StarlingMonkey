//! GC rooting types for SpiderMonkey.
//!
//! Provides stack-based roots that protect GC-managed objects from being
//! collected or having their references go stale during a compacting GC.
//!
//! These types bridge Rust and SpiderMonkey's `Rooted<T>` mechanism via C++
//! shim functions that use placement-new into Rust-allocated stack storage.
//! While a root is alive, it's on SM's root stack — the GC traces and updates
//! it during both nursery and tenured collections.
//!
//! # Usage
//!
//! ```ignore
//! // Root a JSObject*:
//! rooted_object!(in(cx) let obj = sm::sm_new_plain_object(cx));
//! let ptr = obj.get(); // always up-to-date, even after GC
//!
//! // Root a JS::Value:
//! rooted_value!(in(cx) let val = sm::sm_object_value(ptr));
//!
//! // Root a JSString*:
//! rooted_string!(in(cx) let s = sm::sm_new_string_utf8(cx, b, len));
//! ```

use core::ffi::c_void;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use starling_sm_sys as sm;

// ── Size constants ───────────────────────────────────────────────────────────
//
// On wasm32, Rooted<T> layout is:
//   StackRootedBase { stack: **void (4), prev: *void (4) }  → 8 bytes
//   + T data
//
// Rooted<*mut JSObject>: 8 + 4 = 12 bytes, align 4
// Rooted<*mut JSString>: 8 + 4 = 12 bytes, align 4
// Rooted<JS::Value>:     8 + 8 = 16 bytes, align 8

/// Size of `Rooted<*mut JSObject>` / `Rooted<*mut JSString>` on wasm32.
pub const ROOTED_PTR_SIZE: usize = 12;

/// Size of `Rooted<JS::Value>` on wasm32.
pub const ROOTED_VALUE_SIZE: usize = 16;

/// Storage type for `Rooted<*mut JSObject>` or `Rooted<*mut JSString>`.
#[repr(C, align(4))]
pub struct RootedPtrStorage {
    _bytes: [u8; ROOTED_PTR_SIZE],
}

/// Storage type for `Rooted<JS::Value>`.
#[repr(C, align(8))]
pub struct RootedValueStorage {
    _bytes: [u8; ROOTED_VALUE_SIZE],
}

// ── RootedObject ─────────────────────────────────────────────────────────────

/// RAII guard that keeps a `*mut JSObject` rooted on SpiderMonkey's root stack.
///
/// The actual `Rooted<JSObject*>` lives in a `MaybeUninit<RootedPtrStorage>`
/// on the caller's stack (pinned by the borrow). This guard holds a pointer
/// to that storage. Dropping the guard calls the C++ destructor which removes
/// the root from SM's linked list.
pub struct RootedObject<'a> {
    storage: *mut c_void,
    _anchor: PhantomData<&'a mut RootedPtrStorage>,
}

impl<'a> RootedObject<'a> {
    /// Create a new stack root for a JSObject.
    ///
    /// # Safety
    /// `cx` must be a valid JSContext. `initial` must be a valid JSObject*
    /// or null.
    pub unsafe fn new(
        cx: *mut sm::JSContext,
        storage: &'a mut MaybeUninit<RootedPtrStorage>,
        initial: *mut sm::JSObject,
    ) -> Self {
        let ptr = storage.as_mut_ptr() as *mut c_void;
        sm::sm_root_object_init(cx, ptr, initial);
        Self {
            storage: ptr,
            _anchor: PhantomData,
        }
    }

    /// Get the current rooted object pointer.
    /// Always up-to-date, even after a compacting GC.
    pub fn get(&self) -> *mut sm::JSObject {
        unsafe { sm::sm_root_object_get(self.storage as *const c_void) }
    }

    /// Update the rooted value.
    pub fn set(&mut self, obj: *mut sm::JSObject) {
        unsafe { sm::sm_root_object_set(self.storage, obj) }
    }
}

impl Drop for RootedObject<'_> {
    fn drop(&mut self) {
        unsafe { sm::sm_root_object_drop(self.storage) }
    }
}

// ── RootedValue ──────────────────────────────────────────────────────────────

/// RAII guard that keeps a `JS::Value` rooted on SpiderMonkey's root stack.
pub struct RootedValue<'a> {
    storage: *mut c_void,
    _anchor: PhantomData<&'a mut RootedValueStorage>,
}

impl<'a> RootedValue<'a> {
    /// Create a new stack root for a JS::Value.
    ///
    /// # Safety
    /// `cx` must be a valid JSContext.
    pub unsafe fn new(
        cx: *mut sm::JSContext,
        storage: &'a mut MaybeUninit<RootedValueStorage>,
        initial: sm::JSVal,
    ) -> Self {
        let ptr = storage.as_mut_ptr() as *mut c_void;
        sm::sm_root_value_init(cx, ptr, initial);
        Self {
            storage: ptr,
            _anchor: PhantomData,
        }
    }

    /// Get the current rooted value.
    /// Always up-to-date, even after a compacting GC.
    pub fn get(&self) -> sm::JSVal {
        unsafe { sm::sm_root_value_get(self.storage as *const c_void) }
    }

    /// Update the rooted value.
    pub fn set(&mut self, val: sm::JSVal) {
        unsafe { sm::sm_root_value_set(self.storage, val) }
    }
}

impl Drop for RootedValue<'_> {
    fn drop(&mut self) {
        unsafe { sm::sm_root_value_drop(self.storage) }
    }
}

// ── RootedString ─────────────────────────────────────────────────────────────

/// RAII guard that keeps a `*mut JSString` rooted on SpiderMonkey's root stack.
pub struct RootedString<'a> {
    storage: *mut c_void,
    _anchor: PhantomData<&'a mut RootedPtrStorage>,
}

impl<'a> RootedString<'a> {
    /// Create a new stack root for a JSString.
    ///
    /// # Safety
    /// `cx` must be a valid JSContext. `initial` must be a valid JSString*
    /// or null.
    pub unsafe fn new(
        cx: *mut sm::JSContext,
        storage: &'a mut MaybeUninit<RootedPtrStorage>,
        initial: *mut sm::JSString,
    ) -> Self {
        let ptr = storage.as_mut_ptr() as *mut c_void;
        sm::sm_root_string_init(cx, ptr, initial);
        Self {
            storage: ptr,
            _anchor: PhantomData,
        }
    }

    /// Get the current rooted string pointer.
    /// Always up-to-date, even after a compacting GC.
    pub fn get(&self) -> *mut sm::JSString {
        unsafe { sm::sm_root_string_get(self.storage as *const c_void) }
    }

    /// Update the rooted value.
    pub fn set(&mut self, s: *mut sm::JSString) {
        unsafe { sm::sm_root_string_set(self.storage, s) }
    }
}

impl Drop for RootedString<'_> {
    fn drop(&mut self) {
        unsafe { sm::sm_root_string_drop(self.storage) }
    }
}

// ── Convenience macros ───────────────────────────────────────────────────────
//
// Each macro creates a MaybeUninit<Storage> on the stack and a guard that
// borrows it. Shadowing the __root_storage name is safe because the guard
// borrow keeps the old storage alive.

/// Root a `*mut JSObject` on SpiderMonkey's root stack.
///
/// ```ignore
/// rooted_object!(in(cx) let obj = sm::sm_new_plain_object(cx));
/// let p = obj.get();
/// ```
#[macro_export]
macro_rules! rooted_object {
    (in($cx:expr) let $name:ident = $init:expr) => {
        let mut __root_storage =
            ::core::mem::MaybeUninit::<$crate::rooting::RootedPtrStorage>::uninit();
        let $name =
            unsafe { $crate::rooting::RootedObject::new($cx, &mut __root_storage, $init) };
    };
    (in($cx:expr) let mut $name:ident = $init:expr) => {
        let mut __root_storage =
            ::core::mem::MaybeUninit::<$crate::rooting::RootedPtrStorage>::uninit();
        let mut $name =
            unsafe { $crate::rooting::RootedObject::new($cx, &mut __root_storage, $init) };
    };
}

/// Root a `JS::Value` (`JSVal`) on SpiderMonkey's root stack.
///
/// ```ignore
/// rooted_value!(in(cx) let val = sm::sm_object_value(obj));
/// ```
#[macro_export]
macro_rules! rooted_value {
    (in($cx:expr) let $name:ident = $init:expr) => {
        let mut __root_storage =
            ::core::mem::MaybeUninit::<$crate::rooting::RootedValueStorage>::uninit();
        let $name =
            unsafe { $crate::rooting::RootedValue::new($cx, &mut __root_storage, $init) };
    };
    (in($cx:expr) let mut $name:ident = $init:expr) => {
        let mut __root_storage =
            ::core::mem::MaybeUninit::<$crate::rooting::RootedValueStorage>::uninit();
        let mut $name =
            unsafe { $crate::rooting::RootedValue::new($cx, &mut __root_storage, $init) };
    };
}

/// Root a `*mut JSString` on SpiderMonkey's root stack.
///
/// ```ignore
/// rooted_string!(in(cx) let s = sm::sm_new_string_utf8(cx, ptr, len));
/// ```
#[macro_export]
macro_rules! rooted_string {
    (in($cx:expr) let $name:ident = $init:expr) => {
        let mut __root_storage =
            ::core::mem::MaybeUninit::<$crate::rooting::RootedPtrStorage>::uninit();
        let $name =
            unsafe { $crate::rooting::RootedString::new($cx, &mut __root_storage, $init) };
    };
    (in($cx:expr) let mut $name:ident = $init:expr) => {
        let mut __root_storage =
            ::core::mem::MaybeUninit::<$crate::rooting::RootedPtrStorage>::uninit();
        let mut $name =
            unsafe { $crate::rooting::RootedString::new($cx, &mut __root_storage, $init) };
    };
}
