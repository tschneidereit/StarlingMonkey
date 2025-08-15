use std::ffi::CString;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote};
use syn::{parse_macro_input, ItemImpl, ImplItem, ImplItemFn};

#[proc_macro_attribute]
pub fn js_class(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemImpl);
    let ty = &input.self_ty;

    let mut methods = Vec::new();
    for ref mut item in input.items.iter_mut() {
        if let ImplItem::Fn(ref mut method) = item {
            let mut is_method = false;
            method.attrs.retain(|attr| if attr.path().is_ident("method") {
                is_method = true;
                false
            } else {
                true
            });
            if is_method {
                methods.push(function_spec(&ty, method));
            }
        }
    }

    // let class_name = format_ident!("c\"{}\"", quote!(#ty).to_string());
    let class_name = {
        // Derive a simple name (last path segment) or fallback to full token stream
        if let syn::Type::Path(p) = &**ty {
            p.path.segments.last().unwrap().ident.to_string()
        } else {
            quote!(#ty).to_string()
        }
    };
    let class_name = CString::new(class_name).unwrap();
    let class_name = syn::LitCStr::new(&class_name.as_c_str(), Span::call_site());

    // Create the JS_CLASS constant
    let js_class_const: ImplItem = syn::parse_quote! {
        const JS_CLASS: &'static JSClass = &JSClass {
            name: #class_name.as_ptr() as *const i8,
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

    // Generate methods() function
    let methods_fn = generate_methods_function(&methods);

    // Generate static_methods() function
    // let static_methods_fn = generate_static_methods_function(&static_methods, &ty);

    // Insert the constants and functions at the beginning of the impl block

    // let mut ty_item = ItemImpl::new();
    // ty_items.append()

    let class_trait_impl: syn::Item = syn::parse_quote! {
        impl JSClassTrait for #ty {
            #methods_fn
            #js_class_const
            #class_ops_const
        }
    };

    TokenStream::from(quote! { #input #class_trait_impl })
}

// Generate the methods() function
fn generate_methods_function(methods: &[proc_macro2::TokenStream]) -> ImplItem {
    if methods.is_empty() {
        syn::parse_quote! {
            fn methods() -> &'static [JSFunctionSpec] {
                static METHODS: [JSFunctionSpec; 1] = [
                    JSFunctionSpec::ZERO
                ];
                &METHODS
            }
        }
    } else {
        let method_count = methods.len() + 1; // +1 for ZERO terminator
        syn::parse_quote! {
            fn methods() -> &'static [JSFunctionSpec] {
                static METHODS: [JSFunctionSpec; #method_count] = [
                    #(#methods,)*
                    JSFunctionSpec::ZERO
                ];
                &METHODS
            }
        }
    }
}

fn function_spec(class_ty: &syn::Type, method_item: &ImplItemFn) -> proc_macro2::TokenStream {
    let method_name = &method_item.sig.ident;
    // Build a compile-time byte string literal (with NUL terminator) for the method name.
    let cstr = format!("{}\0", method_name);
    let lit = syn::LitByteStr::new(cstr.as_bytes(), Span::call_site());
    quote! {
        JSFunctionSpec {
            name: JSPropertySpec_Name { string_: #lit.as_ptr() as *const i8 },
            call: JSNativeWrapper { op: Some(<#class_ty>::#method_name), info: ptr::null() },
            nargs: 0,
            flags: JSPROP_ENUMERATE as u16,
            selfHostedName: ptr::null(),
        }
    }
}

#[proc_macro]
pub fn error_type(input: TokenStream) -> TokenStream {
    let input = proc_macro2::TokenStream::from(input);
    let mut tokens = input.into_iter();

    // Parse name
    let name = match tokens.next() {
        Some(proc_macro2::TokenTree::Ident(ident)) => ident,
        _ => panic!("Expected identifier for error name"),
    };

    // Skip comma
    tokens.next(); // consume comma

    // Parse exception type
    let exn_type = match tokens.next() {
        Some(proc_macro2::TokenTree::Ident(ident)) => ident,
        _ => panic!("Expected identifier for exception type"),
    };

    // Skip comma
    tokens.next(); // consume comma

    // Parse format string
    let format_string = match tokens.next() {
        Some(proc_macro2::TokenTree::Literal(lit)) => lit,
        _ => panic!("Expected string literal for format"),
    };

    // Count format arguments in the string
    let format_str = format_string.to_string();
    let format_str = format_str.trim_matches('"');

    let mut arg_count = 0;
    let mut chars = format_str.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            if let Some(&next_ch) = chars.peek() {
                if next_ch.is_ascii_digit() {
                    let digit = next_ch.to_digit(10).unwrap() as usize;
                    if digit >= arg_count {
                        arg_count = digit + 1;
                    }
                }
            }
        }
    }

    // Generate constant name (UPPER_SNAKE_CASE)
    let const_name = syn::Ident::new(
        &format!("{}_FORMAT_STRING", name.to_string().to_uppercase()),
        name.span()
    );

    // Generate function name (lower_snake_case)
    let fn_name = syn::Ident::new(
        &format!("throw_{}", to_snake_case(&name.to_string())),
        name.span()
    );

    // Generate the constant
    let const_def = quote! {
        const #const_name: JSErrorFormatString = JSErrorFormatString {
            name: concat!(stringify!(#name), "\0").as_ptr() as *const i8,
            format: concat!(#format_string, "\0").as_ptr() as *const i8,
            argCount: #arg_count as u16,
            exnType: #exn_type as i16,
        };
    };

    // Generate function based on argument count
    let function_def = match arg_count {
        0 => quote! {
            pub fn #fn_name(cx: *mut JSContext) -> bool {
                unsafe {
                    throw_error(
                        cx,
                        &#const_name,
                        std::ptr::null(),
                    )
                }
            }
        },
        1 => quote! {
            pub fn #fn_name(cx: *mut JSContext, arg1: &CStr) -> bool {
                unsafe {
                    throw_error(
                        cx,
                        &#const_name,
                        arg1.as_ptr(),
                        std::ptr::null(),
                        std::ptr::null(),
                        std::ptr::null(),
                    )
                }
            }
        },
        2 => quote! {
            pub fn #fn_name(cx: *mut JSContext, arg1: &CStr, arg2: &CStr) -> bool {
                unsafe {
                    throw_error(
                        cx,
                        &#const_name,
                        arg1.as_ptr(),
                        arg2.as_ptr(),
                        std::ptr::null(),
                        std::ptr::null(),
                    )
                }
            }
        },
        3 => quote! {
            pub fn #fn_name(cx: *mut JSContext, arg1: &CStr, arg2: &CStr, arg3: &CStr) -> bool {
                unsafe {
                    throw_error(
                        cx,
                        &#const_name,
                        arg1.as_ptr(),
                        arg2.as_ptr(),
                        arg3.as_ptr(),
                        std::ptr::null(),
                    )
                }
            }
        },
        4 => quote! {
            pub fn #fn_name(cx: *mut JSContext, arg1: &CStr, arg2: &CStr, arg3: &CStr, arg4: &CStr) -> bool {
                unsafe {
                    throw_error(
                        cx,
                        &#const_name,
                        arg1.as_ptr(),
                        arg2.as_ptr(),
                        arg3.as_ptr(),
                        arg4.as_ptr(),
                    )
                }
            }
        },
        _ => panic!("Unsupported argument count: {}", arg_count),
    };

    let output = quote! {
        #const_def
        #function_def
    };

    TokenStream::from(output)
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch.is_uppercase() {
            if !result.is_empty() {
                result.push('_');
            }
            result.push(ch.to_lowercase().next().unwrap());
        } else {
            result.push(ch);
        }
    }

    result
}
