// Complex concurrent reuse test with timers and multiple nested requests.
//
// Scenario:
// 1. Request A comes in at /
// 2. A starts two concurrent fetches: /worker1 and /worker2
// 3. /worker1 uses setTimeout to delay 10ms, then responds
// 4. /worker2 creates a promise, stores its resolver globally, then awaits it
// 5. A polls (via short timers) until worker2 has set up its resolver,
//    then resolves worker2's promise
// 6. Both workers respond; A collects both and returns combined result
//
// This tests:
// - Multiple concurrent nested requests (3 total requests in flight)
// - Timers (setTimeout) working correctly across concurrent requests
// - Cross-request promise resolution (A resolves worker2's promise)
// - Proper response routing (each request gets its own response)
//
// Note: In p3 with concurrent instance reuse, each request runs in a
// separate handle() call. Workers only start when the current handler
// yields via an async I/O operation, so we use a polling loop instead
// of setTimeout(0) to wait for Worker2 to be ready.

let resolveWorker2;

addEventListener('fetch', async (event) => {
  const url = new URL(event.request.url);

  if (url.pathname === '/worker1') {
    // Worker 1: respond after a short timer
    let resolve;
    event.respondWith(new Promise(r => { resolve = r; }));

    await new Promise(r => setTimeout(r, 10));
    resolve(new Response('worker1-done', { status: 200 }));
    return;
  }

  if (url.pathname === '/worker2') {
    // Worker 2: wait for external resolution from request A
    let resolve;
    event.respondWith(new Promise(r => { resolve = r; }));

    const p = new Promise(r => { resolveWorker2 = r; });
    event.waitUntil(p);
    const val = await p;

    resolve(new Response(`worker2-${val}`, { status: 200 }));
    return;
  }

  // Main request A (path /)
  let resolve;
  event.respondWith(new Promise(r => { resolve = r; }));

  // Start both worker fetches concurrently
  const f1 = fetch('/worker1');
  const f2 = fetch('/worker2');

  // Poll until worker2 has set up its resolver.
  // Each timer yield allows wasmtime to schedule the worker handle() calls.
  while (typeof resolveWorker2 !== 'function') {
    await new Promise(r => setTimeout(r, 1));
  }

  // Resolve worker2's promise from request A's context
  resolveWorker2('resolved-by-A');

  // Collect both responses
  const [r1, r2] = await Promise.all([f1, f2]);
  const [b1, b2] = await Promise.all([r1.text(), r2.text()]);

  resolve(new Response(`${b1}, ${b2}\n`, { status: 200 }));
});
