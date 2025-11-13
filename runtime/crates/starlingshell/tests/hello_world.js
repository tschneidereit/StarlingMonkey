// Basic test script demonstrating console.log functionality
// console.log('Hello from within a global created in Rust with Servo\'s WebIDL bindings, using a WebIDL based Console!');

async function more() {
  await Promise.resolve();
  console.log('after await');
  Promise.resolve().then(() => { console.log('in promise.then'); });
  let { readable, writable } = new TransformStream();
  let writer = writable.getWriter();
  writer.write('data');
  writer.close();
  let reader = readable.getReader();
  let result = await reader.read();
  console.log('read from stream:', result.value);
  let p = fetch('https://example.com/');
  console.log('after fetch call');
  let response = await p;
    console.log('fetched response with status:', response.status);
}

addEventListener("foo", () => {
    console.log('in foo event listener');
    more();
});
  dispatchEvent(new Event("foo"));

setTimeout(() => console.log("timeout"), 2000);
