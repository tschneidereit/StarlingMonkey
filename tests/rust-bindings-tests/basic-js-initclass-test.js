let adder = new Adder();
gc();
console.log(`static: ${Adder.static_add(40, 2)}`);
console.log(`instance: ${adder.add(40, 2.5)}`);
adder = null;
gc();
console.log(`Post GC`);
// Adder.prototype.add(40, 2.5);
