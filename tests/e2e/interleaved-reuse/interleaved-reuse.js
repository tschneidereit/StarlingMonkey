// Interleaved concurrent reuse test.
//
// Scenario:
// 1. Request A comes in at /
// 2. A creates Promise p1, stores resolveP1 in a global
// 3. A triggers nested request A1 (fetch /nested), then awaits p1
// 4. A1 creates Promise p2, stores resolveP2 in a global
// 5. A1 calls resolveP1(), calls event.waitUntil(p2), then awaits p2
// 6. A wakes (p1 resolved), calls resolveP2()
// 7. A1 wakes (p2 resolved), sends its response
// 8. A receives A1's response and forwards it

let resolveP1;
let resolveP2;

addEventListener('fetch', async (event) => {
  const url = new URL(event.request.url);

  if (url.pathname === '/nested') {
    // This is request A1
    let resolve;
    event.respondWith(new Promise(r => { resolve = r; }));

    // Step 4: create p2, store resolveP2
    const p2 = new Promise(r => { resolveP2 = r; });

    // Step 5: resolve p1 (waking A), waitUntil p2, await p2
    resolveP1('p1-resolved');
    event.waitUntil(p2);
    const p2val = await p2;

    // Step 7: send response
    resolve(new Response(`nested: ${p2val}\n`, { status: 200 }));
    return;
  }

  // This is request A (path /)
  let resolve;
  event.respondWith(new Promise(r => { resolve = r; }));

  // Step 2: create p1, store resolveP1
  const p1 = new Promise(r => { resolveP1 = r; });

  // Step 3: trigger nested request A1, then await p1
  const fetchPromise = fetch('/nested');
  const p1val = await p1;

  // Step 6: resolve p2 (waking A1)
  resolveP2('p2-resolved');

  // Step 8: receive A1's response and forward it
  const resp = await fetchPromise;
  const body = await resp.text();
  resolve(new Response(`outer: ${p1val}, ${body}`, { status: 200 }));
});
