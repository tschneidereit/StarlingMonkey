#include "sockets.h"

extern "C" {
int32_t host_api_tcp_socket_make(bool ipv4);
bool host_api_tcp_socket_connect(int32_t handle, uint8_t a, uint8_t b, uint8_t c, uint8_t d,
                                 uint16_t port);
bool host_api_tcp_socket_send(int32_t handle, const uint8_t *data, size_t len);
bool host_api_tcp_socket_receive(int32_t handle, uint32_t chunk_size, uint8_t **out_ptr,
                                 size_t *out_len);
void host_api_tcp_socket_close(int32_t handle);
void host_api_string_free(uint8_t *ptr, size_t len);
}

namespace host_api {
class SocketHandleState final : public HandleState {
  int32_t handle_;
public:
  explicit SocketHandleState(int32_t handle) : handle_(handle) {}
  bool valid() const override { return handle_ >= 0; }
  int32_t handle() const { return handle_; }
  void invalidate() { handle_ = -1; }
};

static int32_t get_socket_handle(HandleState *state) {
  return static_cast<SocketHandleState *>(state)->handle();
}

TCPSocket::TCPSocket(std::unique_ptr<HandleState> state) {
  this->handle_state_ = std::move(state);
}

TCPSocket *TCPSocket::make(IPAddressFamily address_family) {
  auto handle = host_api_tcp_socket_make(address_family == IPV4);
  if (handle < 0) {
    return nullptr;
  }
  return new TCPSocket(std::make_unique<SocketHandleState>(handle));
}

bool TCPSocket::connect(AddressIPV4 address, Port port) {
  auto handle = get_socket_handle(handle_state_.get());
  return host_api_tcp_socket_connect(handle, std::get<0>(address), std::get<1>(address),
                                     std::get<2>(address), std::get<3>(address), port);
}

void TCPSocket::close() {
  if (!valid()) {
    return;
  }
  auto handle = get_socket_handle(handle_state_.get());
  host_api_tcp_socket_close(handle);
  static_cast<SocketHandleState *>(handle_state_.get())->invalidate();
}

bool TCPSocket::send(HostString chunk) {
  auto handle = get_socket_handle(handle_state_.get());
  return host_api_tcp_socket_send(handle, reinterpret_cast<const uint8_t *>(chunk.ptr.get()),
                                  chunk.len);
}

HostString TCPSocket::receive(uint32_t chunk_size) {
  auto handle = get_socket_handle(handle_state_.get());
  uint8_t *ptr = nullptr;
  size_t len = 0;
  if (!host_api_tcp_socket_receive(handle, chunk_size, &ptr, &len)) {
    return HostString(nullptr);
  }
  JS::UniqueChars chars(reinterpret_cast<char *>(ptr));
  return HostString(std::move(chars), len);
}

} // namespace host_api
