//! Callback registry for single-thread mode.
//!
//! This module provides infrastructure for registering callbacks on receivers
//! that get invoked synchronously when messages are sent, bypassing the channel buffer.

use std::any::Any;
use std::boxed::Box;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

/// Type-erased callback that can be stored in the global registry.
/// The actual type is `Arc<Mutex<Box<dyn FnMut(T) + Send>>>` where T is the message type.
type ErasedCallback = Arc<Mutex<Box<dyn Any + Send>>>;

/// Global registry mapping channel addresses (used as IDs) to their callbacks.
static CALLBACK_REGISTRY: LazyLock<Mutex<HashMap<usize, ErasedCallback>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The callback type stored in the registry.
/// Using Arc<Mutex<>> matches the pattern in GenericCallback for consistency.
pub type ReceiverCallback<T> = Arc<Mutex<Box<dyn FnMut(T) + Send>>>;

/// Registers a callback for the given channel address (used as ID).
///
/// The callback will be invoked synchronously when a message is sent to the
/// corresponding sender.
pub(crate) fn register_callback<T: 'static + Send>(id: usize, callback: ReceiverCallback<T>) {
    let erased: ErasedCallback = Arc::new(Mutex::new(Box::new(callback) as Box<dyn Any + Send>));
    CALLBACK_REGISTRY.lock().unwrap().insert(id, erased);
}

/// Unregisters the callback for the given channel address.
pub(crate) fn unregister_callback(id: usize) {
    CALLBACK_REGISTRY.lock().unwrap().remove(&id);
}

/// Attempts to invoke the callback for the given channel address with the provided message.
///
/// Returns `true` if a callback was registered and invoked, `false` otherwise.
pub(crate) fn try_invoke_callback<T: 'static>(id: usize, message: T) -> bool {
    let registry = CALLBACK_REGISTRY.lock().unwrap();
    if let Some(erased) = registry.get(&id) {
        let erased_clone = erased.clone();
        drop(registry); // Release the registry lock before invoking the callback

        let mut guard = erased_clone.lock().unwrap();
        if let Some(callback_arc) = guard.downcast_mut::<ReceiverCallback<T>>() {
            let mut callback_guard = callback_arc.lock().unwrap();
            (*callback_guard)(message);
            return true;
        }
    }
    false
}

/// Checks if a callback is registered for the given channel address.
pub(crate) fn has_callback(id: usize) -> bool {
    CALLBACK_REGISTRY.lock().unwrap().contains_key(&id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_register_and_invoke_callback() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let callback: ReceiverCallback<usize> =
            Arc::new(Mutex::new(Box::new(move |val: usize| {
                counter_clone.fetch_add(val, Ordering::SeqCst);
            })));

        // Use a unique address for this test
        let id = 0x12345678usize;
        register_callback(id, callback);

        assert!(has_callback(id));
        assert!(try_invoke_callback(id, 42usize));
        assert_eq!(counter.load(Ordering::SeqCst), 42);

        // Invoke again
        assert!(try_invoke_callback(id, 8usize));
        assert_eq!(counter.load(Ordering::SeqCst), 50);

        // Unregister
        unregister_callback(id);
        assert!(!has_callback(id));
        assert!(!try_invoke_callback(id, 100usize));
        assert_eq!(counter.load(Ordering::SeqCst), 50); // Unchanged
    }

    #[test]
    fn test_no_callback_registered() {
        let id = 0x87654321usize;
        assert!(!has_callback(id));
        assert!(!try_invoke_callback(id, "test"));
    }
}
