use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemImpl, ImplItem};

#[proc_macro_attribute]
pub fn impl_js_class(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemImpl);

    // Extract the type from the impl block
    let ty = &input.self_ty;

    // Create the JS_CLASS constant
    let js_class_const: ImplItem = syn::parse_quote! {
        const JS_CLASS: &'static JSClass = &JSClass {
            name: <#ty as JSClassTrait>::CLASS_NAME.as_ptr() as *const i8,
            flags: (<#ty as JSClassTrait>::RESERVED_SLOTS << JSCLASS_RESERVED_SLOTS_SHIFT),
            cOps: &<#ty as JSClassTrait>::CLASS_OPS as *const JSClassOps,
            spec: ptr::null(),
            ext: ptr::null(),
            oOps: ptr::null(),
        };
    };

    // Create the CLASS_OPS constant
    let class_ops_const: ImplItem = syn::parse_quote! {
        const CLASS_OPS: JSClassOps = JSClassOps {
            addProperty: None,
            delProperty: None,
            enumerate: None,
            newEnumerate: None,
            resolve: None,
            mayResolve: None,
            finalize: None,
            call: None,
            construct: None,
            trace: None,
        };
    };

    // Insert the constant at the beginning of the impl block
    input.items.insert(0, js_class_const);
    input.items.insert(1, class_ops_const);

    let expanded = quote! {
        #input
    };

    TokenStream::from(expanded)
}
