// Basic test script demonstrating console.log functionality
// console.log('Hello from within a global created in Rust with Servo\'s WebIDL bindings, using a WebIDL based Console!');

async function more() {
  await 1;
  console.log('after await');
  Promise.resolve().then(() => { console.log('in promise.then'); });
}

addEventListener("foo", () => {
    console.log('in foo event listener');
    more();
});
  dispatchEvent(new Event("foo"));

setTimeout(() => console.log("timeout"), 20);
