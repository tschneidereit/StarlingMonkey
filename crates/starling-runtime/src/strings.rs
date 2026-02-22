//! JS string encoding/decoding utilities.
//!
//! Thin wrappers around SpiderMonkey string operations exposed via `starling-sm-sys`.
//! Replaces `runtime/encode.cpp` and `runtime/decode.cpp`.

use core::ffi::c_void;
use starling_sm_sys as sm;

/// Encode a JSString to a UTF-8 byte vector.
///
/// Returns `None` if the encoding fails (e.g., pending exception).
/// The caller is responsible for freeing the returned bytes via `sm_free`.
pub unsafe fn encode_to_utf8(
    cx: *mut sm::JSContext,
    str: *mut sm::JSString,
) -> Option<(*mut u8, usize)> {
    let mut len: u32 = 0;
    let ptr = sm::sm_encode_string_to_utf8(cx, str, &mut len);
    if ptr.is_null() {
        return None;
    }
    Some((ptr, len as usize))
}

/// Encode a JSString to a UTF-8 owned String.
///
/// Automatically frees the SM-allocated buffer after copying.
pub unsafe fn encode_to_string(
    cx: *mut sm::JSContext,
    str: *mut sm::JSString,
) -> Option<String> {
    let (ptr, len) = encode_to_utf8(cx, str)?;
    let bytes = core::slice::from_raw_parts(ptr, len);
    let result = String::from_utf8_lossy(bytes).into_owned();
    sm::sm_free(ptr as *mut c_void);
    Some(result)
}

/// Encode a JSVal (which should be a string value) to a UTF-8 owned String.
pub unsafe fn encode_val_to_string(
    cx: *mut sm::JSContext,
    val: sm::JSVal,
) -> Option<String> {
    if !sm::sm_value_is_string(val) {
        return None;
    }
    let js_str = sm::sm_value_to_string(val);
    if js_str.is_null() {
        return None;
    }
    encode_to_string(cx, js_str)
}

/// Decode a UTF-8 byte slice into a new JSString.
///
/// Returns null if the allocation/conversion fails.
pub unsafe fn decode_utf8(
    cx: *mut sm::JSContext,
    bytes: &[u8],
) -> *mut sm::JSString {
    sm::sm_new_string_utf8(cx, bytes.as_ptr(), bytes.len() as u32)
}

/// Decode a Latin1 byte slice into a new JSString.
///
/// Returns null if the allocation fails.
pub unsafe fn decode_latin1(
    cx: *mut sm::JSContext,
    bytes: &[u8],
) -> *mut sm::JSString {
    sm::sm_new_string_latin1(cx, bytes.as_ptr(), bytes.len() as u32)
}

/// Create a JS string value from a Rust &str.
pub unsafe fn js_string_from_str(
    cx: *mut sm::JSContext,
    s: &str,
) -> sm::JSVal {
    let js_str = sm::sm_new_string_utf8(cx, s.as_ptr(), s.len() as u32);
    if js_str.is_null() {
        return sm::JSVAL_UNDEFINED;
    }
    sm::sm_string_value(js_str)
}
