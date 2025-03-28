var v = "bar";
addEventListener('fetch', (event) => {
    foo(event);
});

function foo(ev) {
    let arr = [1, 2, 3];
    let obj = {
        a: 1,
        b: 2,
        c: 3
    };
    let re = /a/;
    bar(ev);
}

function bar(evt) {
    let body = 'Hello world!';
    console.log(body);
    let resp = new Response(body);
    evt.respondWith(resp);
}
