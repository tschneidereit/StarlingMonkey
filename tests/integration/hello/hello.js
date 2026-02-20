addEventListener('fetch', evt => {
  evt.respondWith(new Response('hello world'));
});
