/**
 * Host API implementation backed by Rust FFI.
 *
 * This file replaces the old wit-bindgen C bindings-based implementation.
 * All WASI interactions are delegated to the rust-host-api crate via extern "C" functions.
 */
#include "host_api.h"

// Forward-declare all Rust FFI functions.
extern "C" {

// poll.rs
void host_api_pollable_block(int32_t handle);
void host_api_pollable_drop(int32_t handle);

// random.rs
struct HostApiBytes {
  uint8_t *ptr;
  size_t len;
};
void host_api_random_get_bytes(size_t len, HostApiBytes *out);
uint32_t host_api_random_get_u32();
void host_api_bytes_free(uint8_t *ptr, size_t len);

// clocks.rs
uint64_t host_api_monotonic_clock_now();
uint64_t host_api_monotonic_clock_resolution();
int32_t host_api_monotonic_clock_subscribe(uint64_t when, bool absolute);

// headers (http_headers.rs)
struct HostApiHeaderEntry {
  uint8_t *name_ptr;
  size_t name_len;
  uint8_t *value_ptr;
  size_t value_len;
};
struct HostApiHeaderEntries {
  HostApiHeaderEntry *entries;
  size_t len;
};
struct HostApiStringEntry {
  uint8_t *ptr;
  size_t len;
};
struct HostApiHeaderValues {
  HostApiStringEntry *values;
  size_t len;
};
int32_t host_api_headers_new();
int32_t host_api_headers_from_entries(const HostApiHeaderEntry *entries, size_t count);
void host_api_headers_entries(int32_t handle, HostApiHeaderEntries *out);
void host_api_header_entries_free(HostApiHeaderEntries *entries);
void host_api_headers_get(int32_t handle, const uint8_t *name_ptr, size_t name_len,
                          HostApiHeaderValues *out);
void host_api_header_values_free(HostApiHeaderValues *values);
bool host_api_headers_has(int32_t handle, const uint8_t *name_ptr, size_t name_len);
bool host_api_headers_set(int32_t handle, const uint8_t *name_ptr, size_t name_len,
                          const uint8_t *value_ptr, size_t value_len);
bool host_api_headers_append(int32_t handle, const uint8_t *name_ptr, size_t name_len,
                             const uint8_t *value_ptr, size_t value_len);
bool host_api_headers_delete(int32_t handle, const uint8_t *name_ptr, size_t name_len);
int32_t host_api_headers_clone(int32_t handle);
void host_api_headers_drop(int32_t handle);

// body (http_body.rs)
struct HostApiReadResult {
  uint8_t *ptr;
  size_t len;
  bool done;
  bool error;
};
void host_api_incoming_body_read(int32_t handle, uint32_t chunk_size, HostApiReadResult *out);
int32_t host_api_incoming_body_subscribe(int32_t handle);
void host_api_incoming_body_close(int32_t handle);
uint64_t host_api_outgoing_body_capacity(int32_t handle);
bool host_api_outgoing_body_write(int32_t handle, const uint8_t *data, size_t len);
int32_t host_api_outgoing_body_subscribe(int32_t handle);
void host_api_outgoing_body_unsubscribe(int32_t handle);
bool host_api_outgoing_body_close(int32_t handle);

// request (http_request.rs)
struct HostApiString {
  uint8_t *ptr;
  size_t len;
};
void host_api_incoming_request_method(int32_t handle, HostApiString *out);
int32_t host_api_incoming_request_headers(int32_t handle);
int32_t host_api_incoming_request_body(int32_t handle);
void host_api_incoming_request_scheme(int32_t handle, HostApiString *out);
void host_api_incoming_request_authority(int32_t handle, HostApiString *out);
void host_api_incoming_request_path_with_query(int32_t handle, HostApiString *out);
void host_api_incoming_request_drop(int32_t handle);
int32_t host_api_outgoing_request_make(const uint8_t *method_ptr, size_t method_len,
                                       int32_t headers_handle, bool has_url,
                                       const uint8_t *scheme_ptr, size_t scheme_len,
                                       const uint8_t *authority_ptr, size_t authority_len,
                                       const uint8_t *path_ptr, size_t path_len);
int32_t host_api_outgoing_request_headers(int32_t handle);
int32_t host_api_outgoing_request_body(int32_t handle);
int32_t host_api_outgoing_request_send(int32_t handle);
int32_t host_api_future_response_subscribe(int32_t handle);
void host_api_future_response_unsubscribe(int32_t handle);
int32_t host_api_future_response_get(int32_t handle);
void host_api_future_response_drop(int32_t handle);

// response (http_response.rs)
uint16_t host_api_incoming_response_status(int32_t handle);
int32_t host_api_incoming_response_headers(int32_t handle);
int32_t host_api_incoming_response_body(int32_t handle);
void host_api_incoming_response_drop(int32_t handle);
int32_t host_api_outgoing_response_make(uint16_t status, int32_t headers_handle);
int32_t host_api_outgoing_response_headers(int32_t handle);
int32_t host_api_outgoing_response_body(int32_t handle);
bool host_api_outgoing_response_send(int32_t handle);

// sockets (sockets.rs)
int32_t host_api_tcp_socket_make(bool ipv4);
bool host_api_tcp_socket_connect(int32_t handle, uint8_t a, uint8_t b, uint8_t c, uint8_t d,
                                 uint16_t port);
bool host_api_tcp_socket_send(int32_t handle, const uint8_t *data, size_t len);
bool host_api_tcp_socket_receive(int32_t handle, uint32_t chunk_size, uint8_t **out_ptr,
                                 size_t *out_len);
void host_api_tcp_socket_close(int32_t handle);

// cli.rs
uint32_t host_api_cli_argc();
void host_api_cli_argv(uint32_t index, uint8_t **out_ptr, size_t *out_len);
void host_api_string_free(uint8_t *ptr, size_t len);

} // extern "C"

// =====================================================================
// HandleState — thin wrapper around an i32 handle
// =====================================================================

static int32_t get_handle(host_api::HandleState *state) {
  return static_cast<host_api::RustHandleState *>(state)->handle();
}

namespace host_api {

// =====================================================================
// Resource
// =====================================================================

Resource::~Resource() {
  handle_state_ = nullptr;
}

bool Resource::valid() const {
  return handle_state_ != nullptr && handle_state_->valid();
}

// =====================================================================
// Random
// =====================================================================

Result<HostBytes> Random::get_bytes(size_t num_bytes) {
  HostApiBytes raw{};
  host_api_random_get_bytes(num_bytes, &raw);
  auto ret = HostBytes{std::unique_ptr<uint8_t[]>(raw.ptr), raw.len};
  return Result<HostBytes>::ok(std::move(ret));
}

Result<uint32_t> Random::get_u32() {
  return Result<uint32_t>::ok(host_api_random_get_u32());
}

// =====================================================================
// MonotonicClock
// =====================================================================

uint64_t MonotonicClock::now() { return host_api_monotonic_clock_now(); }
uint64_t MonotonicClock::resolution() { return host_api_monotonic_clock_resolution(); }

PollableHandle MonotonicClock::subscribe(const uint64_t when, const bool absolute) {
  return host_api_monotonic_clock_subscribe(when, absolute);
}

void MonotonicClock::unsubscribe(const PollableHandle handle_id) {
  host_api_pollable_drop(handle_id);
}

// =====================================================================
// CLI
// =====================================================================

vector<std::string> environment_get_arguments() {
  uint32_t argc = host_api_cli_argc();
  std::vector<std::string> args;
  args.reserve(argc);
  for (uint32_t i = 0; i < argc; i++) {
    uint8_t *ptr = nullptr;
    size_t len = 0;
    host_api_cli_argv(i, &ptr, &len);
    args.emplace_back(reinterpret_cast<char *>(ptr), len);
    host_api_string_free(ptr, len);
  }
  return args;
}

// =====================================================================
// HttpHeaders
// =====================================================================

HttpHeadersReadOnly::HttpHeadersReadOnly() { handle_state_ = nullptr; }

HttpHeadersReadOnly::HttpHeadersReadOnly(std::unique_ptr<HandleState> handle) {
  handle_state_ = std::move(handle);
}

HttpHeaders *HttpHeadersReadOnly::clone() { return new HttpHeaders(*this); }

HttpHeaders::HttpHeaders(std::unique_ptr<HandleState> state)
    : HttpHeadersReadOnly(std::move(state)) {}

HttpHeaders::HttpHeaders() {
  handle_state_ = std::make_unique<RustHandleState>(host_api_headers_new());
}

HttpHeaders::HttpHeaders(const HttpHeadersReadOnly &headers) : HttpHeadersReadOnly(nullptr) {
  auto src_handle = get_handle(headers.handle_state_.get());
  handle_state_ = std::make_unique<RustHandleState>(host_api_headers_clone(src_handle));
}

Result<HttpHeaders *> HttpHeaders::FromEntries(vector<tuple<HostString, HostString>> &entries) {
  std::vector<HostApiHeaderEntry> ffi_entries;
  ffi_entries.reserve(entries.size());
  for (const auto &[name, value] : entries) {
    ffi_entries.push_back({
        reinterpret_cast<uint8_t *>(const_cast<char *>(name.ptr.get())),
        name.len,
        reinterpret_cast<uint8_t *>(const_cast<char *>(value.ptr.get())),
        value.len,
    });
  }

  int32_t handle =
      host_api_headers_from_entries(ffi_entries.data(), ffi_entries.size());
  if (handle < 0) {
    return Result<HttpHeaders *>::err(154);
  }
  return Result<HttpHeaders *>::ok(
      new HttpHeaders(std::make_unique<RustHandleState>(handle)));
}

// Forbidden headers lists
static const std::vector forbidden_request_headers = {
    "connection",          "host",
    "http2-settings",      "keep-alive",
    "proxy-authenticate",  "proxy-authorization",
    "proxy-connection",    "te",
    "transfer-encoding",   "upgrade",
};
static const std::vector forbidden_response_headers = forbidden_request_headers;

const std::vector<const char *> &HttpHeaders::get_forbidden_request_headers() {
  return forbidden_request_headers;
}

const std::vector<const char *> &HttpHeaders::get_forbidden_response_headers() {
  return forbidden_response_headers;
}

Result<vector<tuple<HostString, HostString>>> HttpHeadersReadOnly::entries() const {
  auto handle = get_handle(handle_state_.get());
  HostApiHeaderEntries raw{};
  host_api_headers_entries(handle, &raw);

  vector<tuple<HostString, HostString>> result;
  result.reserve(raw.len);
  for (size_t i = 0; i < raw.len; i++) {
    auto &e = raw.entries[i];
    result.emplace_back(
        HostString(JS::UniqueChars(reinterpret_cast<char *>(e.name_ptr)), e.name_len),
        HostString(JS::UniqueChars(reinterpret_cast<char *>(e.value_ptr)), e.value_len));
  }
  // Free the outer list only — we took ownership of name/value buffers.
  if (raw.entries) {
    free(raw.entries);
  }

  return Result<vector<tuple<HostString, HostString>>>::ok(std::move(result));
}

Result<vector<HostString>> HttpHeadersReadOnly::names() const {
  auto handle = get_handle(handle_state_.get());
  HostApiHeaderEntries raw{};
  host_api_headers_entries(handle, &raw);

  vector<HostString> result;
  result.reserve(raw.len);
  for (size_t i = 0; i < raw.len; i++) {
    auto &e = raw.entries[i];
    result.emplace_back(JS::UniqueChars(reinterpret_cast<char *>(e.name_ptr)), e.name_len);
    // Free value — we don't need it.
    if (e.value_ptr) {
      free(e.value_ptr);
    }
  }
  if (raw.entries) {
    free(raw.entries);
  }

  return Result<vector<HostString>>::ok(std::move(result));
}

Result<optional<vector<HostString>>> HttpHeadersReadOnly::get(string_view name) const {
  auto handle = get_handle(handle_state_.get());
  HostApiHeaderValues raw{};
  host_api_headers_get(handle, reinterpret_cast<const uint8_t *>(name.data()), name.size(), &raw);

  if (raw.len == 0) {
    return Result<optional<vector<HostString>>>::ok(std::nullopt);
  }

  vector<HostString> values;
  values.reserve(raw.len);
  for (size_t i = 0; i < raw.len; i++) {
    auto &v = raw.values[i];
    values.emplace_back(JS::UniqueChars(reinterpret_cast<char *>(v.ptr)), v.len);
  }
  if (raw.values) {
    free(raw.values);
  }

  return Result<optional<vector<HostString>>>::ok(std::move(values));
}

Result<bool> HttpHeadersReadOnly::has(string_view name) const {
  auto handle = get_handle(handle_state_.get());
  return Result<bool>::ok(
      host_api_headers_has(handle, reinterpret_cast<const uint8_t *>(name.data()), name.size()));
}

Result<Void> HttpHeaders::set(string_view name, string_view value) {
  auto handle = get_handle(handle_state_.get());
  if (!host_api_headers_set(handle, reinterpret_cast<const uint8_t *>(name.data()), name.size(),
                            reinterpret_cast<const uint8_t *>(value.data()), value.size())) {
    return Result<Void>::err(154);
  }
  return {};
}

Result<Void> HttpHeaders::append(string_view name, string_view value) {
  auto handle = get_handle(handle_state_.get());
  if (!host_api_headers_append(handle, reinterpret_cast<const uint8_t *>(name.data()), name.size(),
                               reinterpret_cast<const uint8_t *>(value.data()), value.size())) {
    return Result<Void>::err(154);
  }
  return {};
}

Result<Void> HttpHeaders::remove(string_view name) {
  auto handle = get_handle(handle_state_.get());
  if (!host_api_headers_delete(handle, reinterpret_cast<const uint8_t *>(name.data()),
                               name.size())) {
    return Result<Void>::err(154);
  }
  return {};
}

// =====================================================================
// HttpIncomingBody
// =====================================================================

HttpIncomingBody::HttpIncomingBody(std::unique_ptr<HandleState> handle) : Pollable() {
  handle_state_ = std::move(handle);
}

Result<HttpIncomingBody::ReadResult> HttpIncomingBody::read(uint32_t chunk_size) {
  typedef Result<ReadResult> Res;
  auto handle = get_handle(handle_state_.get());
  HostApiReadResult raw{};
  host_api_incoming_body_read(handle, chunk_size, &raw);
  if (raw.error) {
    return Res::err(154);
  }
  return Res::ok(ReadResult(raw.done, unique_ptr<uint8_t[]>(raw.ptr), raw.len));
}

Result<Void> HttpIncomingBody::close() {
  auto handle = get_handle(handle_state_.get());
  host_api_incoming_body_close(handle);
  static_cast<RustHandleState *>(handle_state_.get())->invalidate();
  return {};
}

Result<PollableHandle> HttpIncomingBody::subscribe() {
  auto handle = get_handle(handle_state_.get());
  return Result<PollableHandle>::ok(host_api_incoming_body_subscribe(handle));
}

void HttpIncomingBody::unsubscribe() {
  // Incoming body pollables are returned as raw handles.
  // The caller is expected to drop them via host_api_pollable_drop.
}

// =====================================================================
// HttpOutgoingBody
// =====================================================================

HttpOutgoingBody::HttpOutgoingBody(std::unique_ptr<HandleState> handle) : Pollable() {
  handle_state_ = std::move(handle);
}

Result<uint64_t> HttpOutgoingBody::capacity() {
  if (!valid()) {
    return Result<uint64_t>::err(154);
  }
  auto handle = get_handle(handle_state_.get());
  uint64_t cap = host_api_outgoing_body_capacity(handle);
  return Result<uint64_t>::ok(cap);
}

void HttpOutgoingBody::write(const uint8_t *bytes, size_t len) {
  auto handle = get_handle(handle_state_.get());
  MOZ_RELEASE_ASSERT(host_api_outgoing_body_write(handle, bytes, len));
}

class BodyWriteAllTask final : public api::AsyncTask {
  HttpOutgoingBody *outgoing_body_;
  PollableHandle outgoing_pollable_;

  api::TaskCompletionCallback cb_;
  Heap<JSObject *> cb_receiver_;
  HostBytes bytes_;
  size_t offset_ = 0;

public:
  explicit BodyWriteAllTask(HttpOutgoingBody *outgoing_body, HostBytes bytes,
                            api::TaskCompletionCallback completion_callback,
                            HandleObject callback_receiver)
      : outgoing_body_(outgoing_body), cb_(completion_callback), cb_receiver_(callback_receiver),
        bytes_(std::move(bytes)) {
    outgoing_pollable_ = outgoing_body_->subscribe().unwrap();
  }

  [[nodiscard]] bool run(api::Engine *engine) override {
    MOZ_ASSERT(offset_ < bytes_.len);
    while (true) {
      auto res = outgoing_body_->capacity();
      if (res.is_err()) {
        return false;
      }
      uint64_t cap = res.unwrap();
      if (cap == 0) {
        engine->queue_async_task(this);
        return true;
      }

      auto bytes_to_write = std::min(bytes_.len - offset_, static_cast<size_t>(cap));
      outgoing_body_->write(bytes_.ptr.get() + offset_, bytes_to_write);
      offset_ += bytes_to_write;
      MOZ_ASSERT(offset_ <= bytes_.len);
      if (offset_ == bytes_.len) {
        bytes_.ptr.reset();
        RootedObject receiver(engine->cx(), cb_receiver_);
        bool result = cb_(engine->cx(), receiver);
        cb_ = nullptr;
        cb_receiver_ = nullptr;
        return result;
      }
    }
  }

  [[nodiscard]] bool cancel(api::Engine *engine) override {
    MOZ_ASSERT_UNREACHABLE("BodyWriteAllTask's semantics don't allow for cancellation");
    return true;
  }

  [[nodiscard]] int32_t id() override { return outgoing_pollable_; }

  void trace(JSTracer *trc) override {
    JS::TraceEdge(trc, &cb_receiver_, "BodyWriteAllTask completion callback receiver");
  }
};

Result<Void> HttpOutgoingBody::write_all(api::Engine *engine, HostBytes bytes,
                                         api::TaskCompletionCallback callback,
                                         HandleObject cb_receiver) {
  if (!valid()) {
    return Result<Void>::err(154);
  }
  engine->queue_async_task(new BodyWriteAllTask(this, std::move(bytes), callback, cb_receiver));
  return {};
}

class BodyAppendTask final : public api::AsyncTask {
  enum class State : uint8_t {
    BlockedOnBoth,
    BlockedOnIncoming,
    BlockedOnOutgoing,
    Ready,
    Done,
  };

  HttpIncomingBody *incoming_body_;
  HttpOutgoingBody *outgoing_body_;
  PollableHandle incoming_pollable_;
  PollableHandle outgoing_pollable_;

  api::TaskCompletionCallback cb_;
  Heap<JSObject *> cb_receiver_;
  State state_;

  void set_state(JSContext *cx, const State state) {
    MOZ_ASSERT(state_ != State::Done);
    state_ = state;
    if (state == State::Done && cb_) {
      RootedObject receiver(cx, cb_receiver_);
      cb_(cx, receiver);
      cb_ = nullptr;
      cb_receiver_ = nullptr;
    }
  }

public:
  explicit BodyAppendTask(api::Engine *engine, HttpIncomingBody *incoming_body,
                          HttpOutgoingBody *outgoing_body,
                          api::TaskCompletionCallback completion_callback,
                          HandleObject callback_receiver)
      : incoming_body_(incoming_body), outgoing_body_(outgoing_body), cb_(completion_callback),
        cb_receiver_(callback_receiver), state_(State::BlockedOnBoth) {
    incoming_pollable_ = incoming_body_->subscribe().unwrap();
    outgoing_pollable_ = outgoing_body_->subscribe().unwrap();
  }

  [[nodiscard]] bool run(api::Engine *engine) override {
    if (state_ == State::BlockedOnBoth || state_ == State::BlockedOnIncoming) {
      auto res = incoming_body_->read(0);
      MOZ_ASSERT(!res.is_err());
      auto [done, _] = std::move(res.unwrap());
      if (done) {
        set_state(engine->cx(), State::Done);
        return true;
      }
      set_state(engine->cx(), State::BlockedOnOutgoing);
    }

    MOZ_ASSERT(state_ == State::BlockedOnOutgoing);
    auto res = outgoing_body_->capacity();
    if (res.is_err()) {
      return false;
    }
    uint64_t cap = res.unwrap();
    if (cap > 0) {
      set_state(engine->cx(), State::Ready);
    } else {
      engine->queue_async_task(this);
      return true;
    }

    MOZ_ASSERT(state_ == State::Ready);

    do {
      auto res = incoming_body_->read(cap);
      if (res.is_err()) {
        return false;
      }
      auto [done, bytes] = std::move(res.unwrap());
      if (bytes.len == 0 && !done) {
        set_state(engine->cx(), State::BlockedOnIncoming);
        engine->queue_async_task(this);
        return true;
      }

      if (bytes.len > 0) {
        outgoing_body_->write(bytes.ptr.get(), bytes.len);
      }

      if (done) {
        set_state(engine->cx(), State::Done);
        return true;
      }

      auto capacity_res = outgoing_body_->capacity();
      if (capacity_res.is_err()) {
        return false;
      }
      cap = capacity_res.unwrap();
    } while (cap > 0);

    set_state(engine->cx(), State::BlockedOnOutgoing);
    engine->queue_async_task(this);
    return true;
  }

  [[nodiscard]] bool cancel(api::Engine *engine) override {
    MOZ_ASSERT_UNREACHABLE("BodyAppendTask's semantics don't allow for cancellation");
    return true;
  }

  [[nodiscard]] int32_t id() override {
    if (state_ == State::BlockedOnBoth || state_ == State::BlockedOnIncoming) {
      return incoming_pollable_;
    }
    MOZ_ASSERT(state_ == State::BlockedOnOutgoing);
    return outgoing_pollable_;
  }

  void trace(JSTracer *trc) override {
    JS::TraceEdge(trc, &cb_receiver_, "BodyAppendTask completion callback receiver");
  }
};

Result<Void> HttpOutgoingBody::append(api::Engine *engine, HttpIncomingBody *other,
                                      api::TaskCompletionCallback callback,
                                      HandleObject callback_receiver) {
  engine->queue_async_task(new BodyAppendTask(engine, other, this, callback, callback_receiver));
  return {};
}

Result<Void> HttpOutgoingBody::close() {
  auto handle = get_handle(handle_state_.get());
  host_api_outgoing_body_close(handle);
  static_cast<RustHandleState *>(handle_state_.get())->invalidate();
  return {};
}

Result<PollableHandle> HttpOutgoingBody::subscribe() {
  auto handle = get_handle(handle_state_.get());
  return Result<PollableHandle>::ok(host_api_outgoing_body_subscribe(handle));
}

void HttpOutgoingBody::unsubscribe() {
  auto handle = get_handle(handle_state_.get());
  host_api_outgoing_body_unsubscribe(handle);
}

// =====================================================================
// HttpIncomingRequest
// =====================================================================

HttpIncomingRequest::HttpIncomingRequest(std::unique_ptr<HandleState> handle) {
  handle_state_ = std::move(handle);
}

Result<string_view> HttpIncomingRequest::method() {
  if (method_.empty()) {
    if (!valid()) {
      return Result<string_view>::err(154);
    }
    auto handle = get_handle(handle_state_.get());
    HostApiString raw{};
    host_api_incoming_request_method(handle, &raw);
    method_ = std::string(reinterpret_cast<char *>(raw.ptr), raw.len);
    host_api_string_free(raw.ptr, raw.len);
  }
  return Result<string_view>::ok(method_);
}

Result<HttpHeadersReadOnly *> HttpIncomingRequest::headers() {
  if (!headers_) {
    if (!valid()) {
      return Result<HttpHeadersReadOnly *>::err(154);
    }
    auto handle = get_handle(handle_state_.get());
    auto headers_handle = host_api_incoming_request_headers(handle);
    headers_ = new HttpHeadersReadOnly(std::make_unique<RustHandleState>(headers_handle));
  }
  return Result<HttpHeadersReadOnly *>::ok(headers_);
}

Result<HttpIncomingBody *> HttpIncomingRequest::body() {
  if (!body_) {
    if (!valid()) {
      return Result<HttpIncomingBody *>::err(154);
    }
    auto handle = get_handle(handle_state_.get());
    auto body_handle = host_api_incoming_request_body(handle);
    if (body_handle < 0) {
      return Result<HttpIncomingBody *>::err(154);
    }
    body_ = new HttpIncomingBody(std::make_unique<RustHandleState>(body_handle));
  }
  return Result<HttpIncomingBody *>::ok(body_);
}

// =====================================================================
// HttpRequestResponseBase::url
// =====================================================================

string_view HttpRequestResponseBase::url() {
  if (_url) {
    return string_view(*_url);
  }

  auto handle = get_handle(handle_state_.get());

  HostApiString scheme_raw{}, authority_raw{}, path_raw{};
  host_api_incoming_request_scheme(handle, &scheme_raw);
  host_api_incoming_request_authority(handle, &authority_raw);
  host_api_incoming_request_path_with_query(handle, &path_raw);

  std::string scheme_str(reinterpret_cast<char *>(scheme_raw.ptr), scheme_raw.len);
  std::string authority(reinterpret_cast<char *>(authority_raw.ptr), authority_raw.len);
  std::string path(reinterpret_cast<char *>(path_raw.ptr), path_raw.len);

  host_api_string_free(scheme_raw.ptr, scheme_raw.len);
  host_api_string_free(authority_raw.ptr, authority_raw.len);
  host_api_string_free(path_raw.ptr, path_raw.len);

  _url = new std::string(scheme_str);
  _url->append("://");
  _url->append(authority);
  _url->append(path);

  return string_view(*_url);
}

// =====================================================================
// HttpOutgoingRequest
// =====================================================================

HttpOutgoingRequest::HttpOutgoingRequest(std::unique_ptr<HandleState> state) {
  handle_state_ = std::move(state);
}

HttpOutgoingRequest *HttpOutgoingRequest::make(string_view method_str, optional<HostString> url_str,
                                               std::unique_ptr<HttpHeadersReadOnly> headers) {
  // Create headers in the Rust table.
  auto headers_handle = get_handle(headers->handle_state_.get());
  // Clone the headers into a new handle that OutgoingRequest will own.
  auto owned_headers_handle = host_api_headers_clone(headers_handle);

  bool has_url = url_str.has_value();
  std::string scheme_s, authority_s, path_s;

  if (has_url) {
    jsurl::SpecString val = url_str.value();
    jsurl::JSUrl *url = new_jsurl(&val);
    jsurl::SpecSlice protocol = jsurl::protocol(url);
    scheme_s = std::string(reinterpret_cast<const char *>(protocol.data), protocol.len);
    jsurl::SpecSlice auth = jsurl::authority(url);
    authority_s = std::string(reinterpret_cast<const char *>(auth.data), auth.len);
    jsurl::SpecSlice pq = jsurl::path_with_query(url);
    path_s = std::string(reinterpret_cast<const char *>(pq.data), pq.len);
  }

  auto req_handle = host_api_outgoing_request_make(
      reinterpret_cast<const uint8_t *>(method_str.data()), method_str.size(),
      owned_headers_handle, has_url,
      reinterpret_cast<const uint8_t *>(scheme_s.data()), scheme_s.size(),
      reinterpret_cast<const uint8_t *>(authority_s.data()), authority_s.size(),
      reinterpret_cast<const uint8_t *>(path_s.data()), path_s.size());

  if (req_handle < 0) {
    return nullptr;
  }

  return new HttpOutgoingRequest(std::make_unique<RustHandleState>(req_handle));
}

Result<string_view> HttpOutgoingRequest::method() { return Result<string_view>::ok(method_); }

Result<HttpHeadersReadOnly *> HttpOutgoingRequest::headers() {
  if (!headers_) {
    if (!valid()) {
      return Result<HttpHeadersReadOnly *>::err(154);
    }
    auto handle = get_handle(handle_state_.get());
    auto h = host_api_outgoing_request_headers(handle);
    headers_ = new HttpHeadersReadOnly(std::make_unique<RustHandleState>(h));
  }
  return Result<HttpHeadersReadOnly *>::ok(headers_);
}

Result<HttpOutgoingBody *> HttpOutgoingRequest::body() {
  typedef Result<HttpOutgoingBody *> Res;
  if (!body_) {
    auto handle = get_handle(handle_state_.get());
    auto b = host_api_outgoing_request_body(handle);
    if (b < 0) {
      return Res::err(154);
    }
    body_ = new HttpOutgoingBody(std::make_unique<RustHandleState>(b));
  }
  return Res::ok(body_);
}

Result<FutureHttpIncomingResponse *> HttpOutgoingRequest::send() {
  typedef Result<FutureHttpIncomingResponse *> Res;
  auto handle = get_handle(handle_state_.get());
  auto future_handle = host_api_outgoing_request_send(handle);
  if (future_handle < 0) {
    return Res::err(154);
  }
  static_cast<RustHandleState *>(handle_state_.get())->invalidate();
  return Res::ok(
      new FutureHttpIncomingResponse(std::make_unique<RustHandleState>(future_handle)));
}

// =====================================================================
// FutureHttpIncomingResponse
// =====================================================================

FutureHttpIncomingResponse::FutureHttpIncomingResponse(std::unique_ptr<HandleState> state) {
  handle_state_ = std::move(state);
}

Result<optional<HttpIncomingResponse *>> FutureHttpIncomingResponse::maybe_response() {
  typedef Result<optional<HttpIncomingResponse *>> Res;
  auto handle = get_handle(handle_state_.get());
  auto result = host_api_future_response_get(handle);
  if (result == -1) {
    return Res::ok(std::nullopt); // not ready
  }
  if (result == -2) {
    return Res::err(154); // error
  }
  return Res::ok(new HttpIncomingResponse(std::make_unique<RustHandleState>(result)));
}

Result<PollableHandle> FutureHttpIncomingResponse::subscribe() {
  if (pollable_handle_ == INVALID_POLLABLE_HANDLE) {
    auto handle = get_handle(handle_state_.get());
    pollable_handle_ = host_api_future_response_subscribe(handle);
  }
  return Result<PollableHandle>::ok(pollable_handle_);
}

void FutureHttpIncomingResponse::unsubscribe() {
  if (pollable_handle_ != INVALID_POLLABLE_HANDLE) {
    auto handle = get_handle(handle_state_.get());
    host_api_future_response_unsubscribe(handle);
    pollable_handle_ = INVALID_POLLABLE_HANDLE;
  }
}

// =====================================================================
// HttpIncomingResponse
// =====================================================================

HttpIncomingResponse::HttpIncomingResponse(std::unique_ptr<HandleState> state) {
  handle_state_ = std::move(state);
}

Result<uint16_t> HttpIncomingResponse::status() {
  if (status_ == UNSET_STATUS) {
    if (!valid()) {
      return Result<uint16_t>::err(154);
    }
    auto handle = get_handle(handle_state_.get());
    status_ = host_api_incoming_response_status(handle);
  }
  return Result<uint16_t>::ok(status_);
}

Result<HttpHeadersReadOnly *> HttpIncomingResponse::headers() {
  if (!headers_) {
    if (!valid()) {
      return Result<HttpHeadersReadOnly *>::err(154);
    }
    auto handle = get_handle(handle_state_.get());
    auto h = host_api_incoming_response_headers(handle);
    headers_ = new HttpHeadersReadOnly(std::make_unique<RustHandleState>(h));
  }
  return Result<HttpHeadersReadOnly *>::ok(headers_);
}

Result<HttpIncomingBody *> HttpIncomingResponse::body() {
  if (!body_) {
    if (!valid()) {
      return Result<HttpIncomingBody *>::err(154);
    }
    auto handle = get_handle(handle_state_.get());
    auto b = host_api_incoming_response_body(handle);
    if (b < 0) {
      return Result<HttpIncomingBody *>::err(154);
    }
    body_ = new HttpIncomingBody(std::make_unique<RustHandleState>(b));
  }
  return Result<HttpIncomingBody *>::ok(body_);
}

// =====================================================================
// HttpOutgoingResponse
// =====================================================================

HttpOutgoingResponse::HttpOutgoingResponse(std::unique_ptr<HandleState> state) {
  handle_state_ = std::move(state);
}

HttpOutgoingResponse *HttpOutgoingResponse::make(const uint16_t status,
                                                 unique_ptr<HttpHeaders> headers) {
  auto headers_handle = get_handle(headers->handle_state_.get());
  // Clone the headers since make takes ownership.
  auto owned_headers = host_api_headers_clone(headers_handle);
  auto handle = host_api_outgoing_response_make(status, owned_headers);
  if (handle < 0) {
    return nullptr;
  }
  auto *resp = new HttpOutgoingResponse(std::make_unique<RustHandleState>(handle));
  resp->status_ = status;
  return resp;
}

Result<HttpHeadersReadOnly *> HttpOutgoingResponse::headers() {
  if (!headers_) {
    if (!valid()) {
      return Result<HttpHeadersReadOnly *>::err(154);
    }
    auto handle = get_handle(handle_state_.get());
    auto h = host_api_outgoing_response_headers(handle);
    headers_ = new HttpHeadersReadOnly(std::make_unique<RustHandleState>(h));
  }
  return Result<HttpHeadersReadOnly *>::ok(headers_);
}

Result<HttpOutgoingBody *> HttpOutgoingResponse::body() {
  typedef Result<HttpOutgoingBody *> Res;
  if (!body_) {
    auto handle = get_handle(handle_state_.get());
    auto b = host_api_outgoing_response_body(handle);
    if (b < 0) {
      return Res::err(154);
    }
    body_ = new HttpOutgoingBody(std::make_unique<RustHandleState>(b));
  }
  return Res::ok(body_);
}

Result<uint16_t> HttpOutgoingResponse::status() {
  return Result<uint16_t>::ok(status_);
}

Result<Void> HttpOutgoingResponse::send() {
  auto handle = get_handle(handle_state_.get());
  if (!host_api_outgoing_response_send(handle)) {
    return Result<Void>::err(154);
  }
  static_cast<RustHandleState *>(handle_state_.get())->invalidate();
  return {};
}

// =====================================================================
// Request handler (exports integration)
// =====================================================================

static HttpIncomingRequest::RequestHandler REQUEST_HANDLER = nullptr;

void HttpIncomingRequest::set_handler(RequestHandler handler) {
  MOZ_ASSERT(!REQUEST_HANDLER);
  REQUEST_HANDLER = handler;
}

void block_on_pollable_handle(PollableHandle handle) {
  host_api_pollable_block(handle);
}

} // namespace host_api
