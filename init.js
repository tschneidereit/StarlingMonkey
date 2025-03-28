const builtin = {
    foo() {
        return 'foo';
    }
}

defineBuiltinModule('builtin', builtin);
print("done");
