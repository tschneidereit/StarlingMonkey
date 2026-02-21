use crate::handle_table::HandleTable;
use wasi::http::types::Fields;

use core::cell::RefCell;

thread_local! {
    static HEADERS_TABLE: RefCell<HandleTable<Fields>> = const { RefCell::new(HandleTable::new()) };
}

fn with_headers<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<Fields>) -> R,
{
    HEADERS_TABLE.with(|t| f(&mut t.borrow_mut()))
}

/// Insert a Fields into the shared headers table. Used by other modules.
pub(crate) fn insert_headers(fields: Fields) -> i32 {
    with_headers(|t| t.insert(fields))
}

/// Remove and return a Fields from the shared headers table.
pub(crate) fn remove_headers(handle: i32) -> Option<Fields> {
    with_headers(|t| t.remove(handle))
}

/// FFI struct for a single header entry (name + value).
#[repr(C)]
pub struct HostApiHeaderEntry {
    pub name_ptr: *mut u8,
    pub name_len: usize,
    pub value_ptr: *mut u8,
    pub value_len: usize,
}

/// FFI struct for a list of header entries.
#[repr(C)]
pub struct HostApiHeaderEntries {
    pub entries: *mut HostApiHeaderEntry,
    pub len: usize,
}

/// FFI struct for a list of header values (for get()).
#[repr(C)]
pub struct HostApiHeaderValues {
    pub values: *mut HostApiStringEntry,
    pub len: usize,
}

/// FFI struct for a single string.
#[repr(C)]
pub struct HostApiStringEntry {
    pub ptr: *mut u8,
    pub len: usize,
}

/// Create a new empty Fields (headers) resource.
/// Returns a handle (i32) to the new Fields.
#[no_mangle]
pub extern "C" fn host_api_headers_new() -> i32 {
    let fields = Fields::new();
    with_headers(|t| t.insert(fields))
}

/// Create Fields from a list of name/value pairs.
/// Returns a handle on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn host_api_headers_from_entries(
    entries: *const HostApiHeaderEntry,
    count: usize,
) -> i32 {
    let entries_slice = core::slice::from_raw_parts(entries, count);
    let pairs: Vec<(String, Vec<u8>)> = entries_slice
        .iter()
        .map(|e| {
            let name =
                core::str::from_utf8_unchecked(core::slice::from_raw_parts(e.name_ptr, e.name_len))
                    .to_string();
            let value = core::slice::from_raw_parts(e.value_ptr, e.value_len).to_vec();
            (name, value)
        })
        .collect();

    match Fields::from_list(&pairs) {
        Ok(fields) => with_headers(|t| t.insert(fields)),
        Err(_) => -1,
    }
}

/// Get all entries from a Fields handle.
/// Caller must free the returned entries with host_api_header_entries_free.
#[no_mangle]
pub extern "C" fn host_api_headers_entries(handle: i32, out: *mut HostApiHeaderEntries) {
    with_headers(|t| {
        let fields = t.get(handle).expect("invalid headers handle");
        let entries = fields.entries();
        let mut ffi_entries: Vec<HostApiHeaderEntry> = entries
            .into_iter()
            .map(|(name, value)| {
                let (name_ptr, name_len) = if name.is_empty() {
                    (core::ptr::null_mut(), 0)
                } else {
                    let name_bytes = name.into_bytes().into_boxed_slice();
                    let p = name_bytes.as_ptr() as *mut u8;
                    let l = name_bytes.len();
                    core::mem::forget(name_bytes);
                    (p, l)
                };
                let (value_ptr, value_len) = if value.is_empty() {
                    (core::ptr::null_mut(), 0)
                } else {
                    let value_bytes = value.into_boxed_slice();
                    let p = value_bytes.as_ptr() as *mut u8;
                    let l = value_bytes.len();
                    core::mem::forget(value_bytes);
                    (p, l)
                };
                HostApiHeaderEntry {
                    name_ptr,
                    name_len,
                    value_ptr,
                    value_len,
                }
            })
            .collect();

        let result = HostApiHeaderEntries {
            entries: ffi_entries.as_mut_ptr(),
            len: ffi_entries.len(),
        };
        core::mem::forget(ffi_entries);
        unsafe { *out = result };
    });
}

/// Free header entries returned by host_api_headers_entries.
#[no_mangle]
pub unsafe extern "C" fn host_api_header_entries_free(entries: *mut HostApiHeaderEntries) {
    if entries.is_null() {
        return;
    }
    let e = &*entries;
    if !e.entries.is_null() && e.len > 0 {
        let vec = Vec::from_raw_parts(e.entries, e.len, e.len);
        for entry in vec {
            if !entry.name_ptr.is_null() {
                let _ = Vec::from_raw_parts(entry.name_ptr, entry.name_len, entry.name_len);
            }
            if !entry.value_ptr.is_null() {
                let _ = Vec::from_raw_parts(entry.value_ptr, entry.value_len, entry.value_len);
            }
        }
    }
}

/// Get values for a specific header name.
/// Returns the values in `out`. If the header doesn't exist, out.len == 0.
#[no_mangle]
pub unsafe extern "C" fn host_api_headers_get(
    handle: i32,
    name_ptr: *const u8,
    name_len: usize,
    out: *mut HostApiHeaderValues,
) {
    let name = core::str::from_utf8_unchecked(core::slice::from_raw_parts(name_ptr, name_len));
    with_headers(|t| {
        let fields = t.get(handle).expect("invalid headers handle");
        let values = fields.get(name);
        let mut ffi_values: Vec<HostApiStringEntry> = values
            .into_iter()
            .map(|v| {
                if v.is_empty() {
                    HostApiStringEntry {
                        ptr: core::ptr::null_mut(),
                        len: 0,
                    }
                } else {
                    let boxed = v.into_boxed_slice();
                    let entry = HostApiStringEntry {
                        ptr: boxed.as_ptr() as *mut u8,
                        len: boxed.len(),
                    };
                    core::mem::forget(boxed);
                    entry
                }
            })
            .collect();

        let result = HostApiHeaderValues {
            values: ffi_values.as_mut_ptr(),
            len: ffi_values.len(),
        };
        core::mem::forget(ffi_values);
        *out = result;
    });
}

/// Free header values returned by host_api_headers_get.
#[no_mangle]
pub unsafe extern "C" fn host_api_header_values_free(values: *mut HostApiHeaderValues) {
    if values.is_null() {
        return;
    }
    let v = &*values;
    if !v.values.is_null() && v.len > 0 {
        let vec = Vec::from_raw_parts(v.values, v.len, v.len);
        for entry in vec {
            if !entry.ptr.is_null() {
                let _ = Vec::from_raw_parts(entry.ptr, entry.len, entry.len);
            }
        }
    }
}

/// Check if a header name exists.
#[no_mangle]
pub unsafe extern "C" fn host_api_headers_has(
    handle: i32,
    name_ptr: *const u8,
    name_len: usize,
) -> bool {
    let name = core::str::from_utf8_unchecked(core::slice::from_raw_parts(name_ptr, name_len));
    with_headers(|t| {
        let fields = t.get(handle).expect("invalid headers handle");
        fields.has(name)
    })
}

/// Set a header (replaces all values for that name).
/// Returns true on success, false on error.
#[no_mangle]
pub unsafe extern "C" fn host_api_headers_set(
    handle: i32,
    name_ptr: *const u8,
    name_len: usize,
    value_ptr: *const u8,
    value_len: usize,
) -> bool {
    let name = core::str::from_utf8_unchecked(core::slice::from_raw_parts(name_ptr, name_len));
    let value = core::slice::from_raw_parts(value_ptr, value_len);
    with_headers(|t| {
        let fields = t.get(handle).expect("invalid headers handle");
        fields.set(name, &[value.to_vec()]).is_ok()
    })
}

/// Append a header value.
/// Returns true on success, false on error.
#[no_mangle]
pub unsafe extern "C" fn host_api_headers_append(
    handle: i32,
    name_ptr: *const u8,
    name_len: usize,
    value_ptr: *const u8,
    value_len: usize,
) -> bool {
    let name = core::str::from_utf8_unchecked(core::slice::from_raw_parts(name_ptr, name_len));
    let value = core::slice::from_raw_parts(value_ptr, value_len);
    with_headers(|t| {
        let fields = t.get(handle).expect("invalid headers handle");
        fields.append(name, value).is_ok()
    })
}

/// Delete a header by name.
/// Returns true on success, false on error.
#[no_mangle]
pub unsafe extern "C" fn host_api_headers_delete(
    handle: i32,
    name_ptr: *const u8,
    name_len: usize,
) -> bool {
    let name = core::str::from_utf8_unchecked(core::slice::from_raw_parts(name_ptr, name_len));
    with_headers(|t| {
        let fields = t.get(handle).expect("invalid headers handle");
        fields.delete(name).is_ok()
    })
}

/// Clone a Fields handle.
/// Returns a new handle to the cloned Fields.
#[no_mangle]
pub extern "C" fn host_api_headers_clone(handle: i32) -> i32 {
    with_headers(|t| {
        let fields = t.get(handle).expect("invalid headers handle");
        let cloned = fields.clone();
        t.insert(cloned)
    })
}

/// Drop a Fields handle, releasing the underlying resource.
#[no_mangle]
pub extern "C" fn host_api_headers_drop(handle: i32) {
    with_headers(|t| {
        t.remove(handle);
    });
}

/// Take ownership of a Fields handle out of the table, returning the raw WASI handle.
/// After this call, the handle is no longer valid in the table.
/// Used when passing Fields ownership to another WASI resource (e.g. OutgoingRequest constructor).
#[no_mangle]
pub extern "C" fn host_api_headers_take_handle(handle: i32) -> u32 {
    with_headers(|t| {
        let fields = t.remove(handle).expect("invalid headers handle");
        let raw = fields.handle();
        core::mem::forget(fields); // Don't drop — ownership transferred.
        raw
    })
}
