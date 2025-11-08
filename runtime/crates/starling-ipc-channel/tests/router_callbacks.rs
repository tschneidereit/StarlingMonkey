// Test that router callbacks work synchronously in inprocess mode

#![cfg(feature = "single-thread")]

#[test]
fn test_router_callback_is_synchronous() {
    use ipc_channel::ipc;
    use ipc_channel::router::ROUTER;
    use std::sync::{Arc, Mutex};

    let (tx, rx) = ipc::channel().unwrap();
    let called = Arc::new(Mutex::new(false));
    let called_clone = called.clone();

    ROUTER.add_typed_route(
        rx,
        Box::new(move |msg: Result<i32, _>| {
            *called_clone.lock().unwrap() = true;
            assert_eq!(msg.unwrap(), 42);
        }),
    );

    tx.send(42).unwrap();

    // Should be called synchronously
    assert!(*called.lock().unwrap());
}

#[test]
fn test_multiple_senders_to_same_receiver() {
    use ipc_channel::ipc;
    use ipc_channel::router::ROUTER;
    use std::sync::{Arc, Mutex};

    let (tx1, rx) = ipc::channel().unwrap();
    let tx2 = tx1.clone();

    let messages = Arc::new(Mutex::new(Vec::new()));
    let messages_clone = messages.clone();

    ROUTER.add_typed_route(
        rx,
        Box::new(move |msg: Result<i32, _>| {
            messages_clone.lock().unwrap().push(msg.unwrap());
        }),
    );

    tx1.send(1).unwrap();
    tx2.send(2).unwrap();

    assert_eq!(messages.lock().unwrap().as_slice(), &[1, 2]);
}

#[test]
fn test_route_to_crossbeam_channel() {
    use ipc_channel::ipc;
    use ipc_channel::router::ROUTER;

    let (tx, rx) = ipc::channel::<String>().unwrap();

    // Route IPC channel to crossbeam channel
    let crossbeam_rx = ROUTER.route_ipc_receiver_to_new_crossbeam_receiver(rx);

    tx.send("hello".to_string()).unwrap();

    // Should be able to receive from crossbeam channel
    let received = crossbeam_rx.recv().unwrap();
    assert_eq!(received, "hello");
}
