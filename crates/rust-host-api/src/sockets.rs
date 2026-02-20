use crate::handle_table::HandleTable;
use wasi::io::poll::Pollable;
use wasi::io::streams::{InputStream, OutputStream};
use wasi::sockets::instance_network;
use wasi::sockets::network::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress};
use wasi::sockets::tcp::TcpSocket;
use wasi::sockets::tcp_create_socket;

use core::cell::RefCell;

const INVALID_POLLABLE_HANDLE: i32 = -1;

struct TcpSocketState {
    socket: TcpSocket,
    pollable_handle: i32,
    input: Option<InputStream>,
    output: Option<OutputStream>,
}

thread_local! {
    static SOCKET_TABLE: RefCell<HandleTable<TcpSocketState>> = RefCell::new(HandleTable::new());
}

fn with_sockets<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandleTable<TcpSocketState>) -> R,
{
    SOCKET_TABLE.with(|t| f(&mut t.borrow_mut()))
}

/// Create a new TCP socket.
/// `ipv4` — true for IPv4, false for IPv6.
/// Returns a socket handle, or -1 on error.
#[no_mangle]
pub extern "C" fn host_api_tcp_socket_make(ipv4: bool) -> i32 {
    let family = if ipv4 {
        IpAddressFamily::Ipv4
    } else {
        IpAddressFamily::Ipv6
    };
    match tcp_create_socket::create_tcp_socket(family) {
        Ok(socket) => {
            let state = TcpSocketState {
                socket,
                pollable_handle: INVALID_POLLABLE_HANDLE,
                input: None,
                output: None,
            };
            with_sockets(|t| t.insert(state))
        }
        Err(_) => -1,
    }
}

/// Connect a TCP socket to an IPv4 address.
/// Returns true on success.
#[no_mangle]
pub extern "C" fn host_api_tcp_socket_connect(
    handle: i32,
    a: u8,
    b: u8,
    c: u8,
    d: u8,
    port: u16,
) -> bool {
    with_sockets(|t| {
        let state = t.get_mut(handle).expect("invalid socket handle");
        let network = instance_network::instance_network();
        let addr = IpSocketAddress::Ipv4(Ipv4SocketAddress {
            port,
            address: (a, b, c, d),
        });

        if state.socket.start_connect(&network, addr).is_err() {
            return false;
        }

        // Get pollable for blocking.
        let pollable = state.socket.subscribe();

        loop {
            match state.socket.finish_connect() {
                Ok((input, output)) => {
                    state.input = Some(input);
                    state.output = Some(output);
                    drop(pollable);
                    return true;
                }
                Err(e) => {
                    if e == wasi::sockets::network::ErrorCode::WouldBlock {
                        pollable.block();
                        continue;
                    }
                    drop(pollable);
                    return false;
                }
            }
        }
    })
}

/// Send data over a connected TCP socket.
/// Returns true on success.
#[no_mangle]
pub unsafe extern "C" fn host_api_tcp_socket_send(
    handle: i32,
    data: *const u8,
    len: usize,
) -> bool {
    let bytes = core::slice::from_raw_parts(data, len);
    with_sockets(|t| {
        let state = t.get(handle).expect("invalid socket handle");
        let output = state.output.as_ref().expect("socket not connected");
        // Check capacity first.
        match output.check_write() {
            Ok(cap) if cap >= len as u64 => output.write(bytes).is_ok(),
            _ => false,
        }
    })
}

/// Receive data from a connected TCP socket.
/// Results are written to the out parameters.
#[no_mangle]
pub extern "C" fn host_api_tcp_socket_receive(
    handle: i32,
    chunk_size: u32,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> bool {
    with_sockets(|t| {
        let state = t.get(handle).expect("invalid socket handle");
        let input = state.input.as_ref().expect("socket not connected");
        match input.blocking_read(chunk_size as u64) {
            Ok(bytes) => {
                let mut boxed = bytes.into_boxed_slice();
                unsafe {
                    *out_ptr = boxed.as_mut_ptr();
                    *out_len = boxed.len();
                }
                core::mem::forget(boxed);
                true
            }
            Err(_) => false,
        }
    })
}

/// Close a TCP socket.
#[no_mangle]
pub extern "C" fn host_api_tcp_socket_close(handle: i32) {
    with_sockets(|t| {
        if let Some(mut state) = t.remove(handle) {
            // Shut down the socket.
            let _ = state
                .socket
                .shutdown(wasi::sockets::tcp::ShutdownType::Both);

            // Drop streams.
            state.output.take();
            state.input.take();

            // Drop pollable if any.
            if state.pollable_handle != INVALID_POLLABLE_HANDLE {
                let pollable =
                    unsafe { Pollable::from_handle(state.pollable_handle as u32) };
                drop(pollable);
            }
            // Socket is dropped automatically.
        }
    });
}
