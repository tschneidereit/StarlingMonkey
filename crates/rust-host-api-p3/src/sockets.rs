use crate::handle_table::HandleTable;
use wasip3::sockets::types::{IpAddressFamily, IpSocketAddress, Ipv4SocketAddress, TcpSocket};
// block_on is used here because the debugger protocol is inherently synchronous:
// the debugger client sends a command, we must read/process/respond before
// continuing. These operations happen at pause points where no other async
// tasks need to make progress.
use wit_bindgen::rt::async_support::{block_on, StreamReader, StreamWriter};

use core::cell::RefCell;

struct TcpSocketState {
    socket: TcpSocket,
    input: Option<StreamReader<u8>>,
    output: Option<StreamWriter<u8>>,
}

thread_local! {
    static SOCKET_TABLE: RefCell<HandleTable<TcpSocketState>> = const { RefCell::new(HandleTable::new()) };
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
    match TcpSocket::create(family) {
        Ok(socket) => {
            let state = TcpSocketState {
                socket,
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
///
/// In WASIp3, socket connect is async. We use block_on to yield to
/// wasmtime's event loop for the connection handshake.
#[no_mangle]
pub extern "C" fn host_api_tcp_socket_connect(
    handle: i32,
    a: u8,
    b: u8,
    c: u8,
    d: u8,
    port: u16,
) -> bool {
    // Take the socket raw handle to avoid borrowing across block_on.
    let socket_raw = with_sockets(|t| {
        let state = t.get(handle).expect("invalid socket handle");
        state.socket.handle()
    });

    let addr = IpSocketAddress::Ipv4(Ipv4SocketAddress {
        port,
        address: (a, b, c, d),
    });

    // Temporarily wrap the raw handle to call connect.
    // SAFETY: We forget this after use to avoid double-free.
    let temp_socket = unsafe { TcpSocket::from_handle(socket_raw) };
    let result = block_on(temp_socket.connect(addr));

    if result.is_ok() {
        // Get receive stream.
        let (reader, _err_future) = temp_socket.receive();
        core::mem::forget(temp_socket);

        with_sockets(|t| {
            let state = t.get_mut(handle).expect("invalid socket handle");
            state.input = Some(reader);
        });
        true
    } else {
        core::mem::forget(temp_socket);
        false
    }
}

/// Send data over a connected TCP socket.
/// Returns true on success.
///
/// In WASIp3, send takes a StreamReader and is async. For each send call,
/// we create a one-shot stream, write data via block_on, and block on send.
#[no_mangle]
pub unsafe extern "C" fn host_api_tcp_socket_send(
    handle: i32,
    data: *const u8,
    len: usize,
) -> bool {
    let bytes = core::slice::from_raw_parts(data, len).to_vec();

    // Get the raw socket handle without holding the borrow.
    let socket_raw = with_sockets(|t| {
        let state = t.get(handle).expect("invalid socket handle");
        state.socket.handle()
    });

    // Create a one-shot stream for this send.
    // wit_stream::new returns (StreamWriter, StreamReader).
    let (mut writer, reader) = wasip3::wit_stream::new::<u8>();

    // Write the data to the stream using block_on (async write).
    let remaining = block_on(writer.write_all(bytes));
    drop(writer); // Close the write end so send() can complete.

    if !remaining.is_empty() {
        return false;
    }

    // Temporarily wrap the raw handle.
    let temp_socket = TcpSocket::from_handle(socket_raw);
    let result = block_on(temp_socket.send(reader));
    core::mem::forget(temp_socket);

    result.is_ok()
}

/// Receive data from a connected TCP socket.
///
/// In WASIp3, StreamReader::next() is async. We use block_on() which
/// yields to wasmtime for I/O scheduling. This blocks until data arrives.
#[no_mangle]
pub extern "C" fn host_api_tcp_socket_receive(
    handle: i32,
    chunk_size: u32,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> bool {
    // Take the reader out to avoid borrowing across block_on.
    let reader_opt = with_sockets(|t| {
        let state = t.get_mut(handle).expect("invalid socket handle");
        state.input.take()
    });

    if let Some(mut reader) = reader_opt {
        // Read bytes one at a time until we have chunk_size or stream ends.
        let mut buf = Vec::with_capacity(chunk_size as usize);

        // Read first byte (blocks via block_on → yields to wasmtime).
        match block_on(reader.next()) {
            Some(byte) => {
                buf.push(byte);
                // Try to read more bytes up to chunk_size without blocking
                // by doing additional block_on calls (each yields to wasmtime
                // but returns quickly if data is already buffered).
                while buf.len() < chunk_size as usize {
                    // Use a read with a buffer to get more data at once.
                    let capacity = (chunk_size as usize - buf.len()).min(4096);
                    let read_buf = Vec::with_capacity(capacity);
                    let (_result, data) = block_on(reader.read(read_buf));
                    if data.is_empty() {
                        break;
                    }
                    buf.extend_from_slice(&data);
                }
            }
            None => {
                // Stream ended
                with_sockets(|t| {
                    let state = t.get_mut(handle).expect("invalid socket handle");
                    state.input = Some(reader);
                });
                return false;
            }
        }

        // Put the reader back.
        with_sockets(|t| {
            let state = t.get_mut(handle).expect("invalid socket handle");
            state.input = Some(reader);
        });

        if buf.is_empty() {
            return false;
        }

        let mut boxed = buf.into_boxed_slice();
        unsafe {
            *out_ptr = boxed.as_mut_ptr();
            *out_len = boxed.len();
        }
        core::mem::forget(boxed);
        true
    } else {
        false
    }
}

/// Close a TCP socket.
#[no_mangle]
pub extern "C" fn host_api_tcp_socket_close(handle: i32) {
    with_sockets(|t| {
        if let Some(mut state) = t.remove(handle) {
            state.output.take();
            state.input.take();
            // Socket is dropped automatically.
        }
    });
}
