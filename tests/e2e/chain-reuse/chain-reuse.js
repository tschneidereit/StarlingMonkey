// Chain-of-three concurrent reuse test.
//
// Request chain: A -> B -> C, with promise resolution propagating back.
//
// 1. A fetches /step2
// 2. B (handling /step2) fetches /step3, stores resolveB globally, awaits promiseB
// 3. C (handling /step3) resolves B's promise, stores resolveC globally, awaits promiseC
// 4. B wakes, resolves C's promise, sends its response
// 5. C wakes, sends its response
// 6. B receives C's response, incorporates it into B's response
// 7. A receives B's response, returns final result
//
// Tests 3-deep nested request chains with cross-request promise resolution.

let resolveB;
let resolveC;

addEventListener('fetch', async (event) => {
  const url = new URL(event.request.url);

  if (url.pathname === '/step3') {
    // Request C: resolve B, wait for own resolution from B
    let resolve;
    event.respondWith(new Promise(r => { resolve = r; }));

    // Resolve B's promise
    resolveB('B-unblocked');

    // Create C's own blocking promise
    const promiseC = new Promise(r => { resolveC = r; });
    event.waitUntil(promiseC);
    const cVal = await promiseC;

    resolve(new Response(`C:${cVal}`, { status: 200 }));
    return;
  }

  if (url.pathname === '/step2') {
    // Request B: fetch C, wait for resolution from C, then resolve C
    let resolve;
    event.respondWith(new Promise(r => { resolve = r; }));

    // Create B's blocking promise
    const promiseB = new Promise(r => { resolveB = r; });

    // Fetch step3 (will create request C)
    const fetchC = fetch('/step3');

    // Wait for C to resolve our promise
    event.waitUntil(promiseB);
    const bVal = await promiseB;

    // Now resolve C's promise
    resolveC('C-unblocked');

    // Get C's response
    const respC = await fetchC;
    const bodyC = await respC.text();

    resolve(new Response(`B:${bVal},${bodyC}`, { status: 200 }));
    return;
  }

  // Request A (path /): fetch step2 and return result
  let resolve;
  event.respondWith(new Promise(r => { resolve = r; }));

  const resp = await fetch('/step2');
  const body = await resp.text();

  resolve(new Response(`A:${body}\n`, { status: 200 }));
});
