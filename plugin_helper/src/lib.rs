use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields};

#[proc_macro_derive(PluginHelper)]
pub fn plugin_helper(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    // Extract the identifier (name) of the struct
    let name = input.ident;

    // Initialize a variable to hold generated code for each field's dd call
    let mut dump_state_calls = Vec::new();
    let mut on_translation_calls = Vec::new();
    let mut init_calls = Vec::new();

    // Match against the data of the struct to access its fields
    if let Data::Struct(data) = input.data {
        if let Fields::Named(fields) = data.fields {
            // Iterate over the struct's fields
            for field in fields.named {
                let ty = &field.ty;
                // Assuming the field type implements DD, generate a dd call
                dump_state_calls.push(quote! {
                    #ty::dump_snapshot(name);
                });
                on_translation_calls.push(quote! {
                    #ty::on_translation(tb);
                });
                init_calls.push(quote! {
                    #ty::init();
                });
            }
        }
    }

    // Generate the implementation of the dd function for the struct
    let expanded = quote! {
        impl #name {
            #[inline]
            pub unsafe fn dump_snapshot(name: &str) {
                #( #dump_state_calls )*
            }

            #[inline]
            pub unsafe fn on_translation(tb: *mut crate::qemu_api::qemu_plugin_tb) {
                #( #on_translation_calls )*
            }

            #[inline]
            pub unsafe fn init() {
                #( #init_calls )*
            }
        }
    };

    TokenStream::from(expanded)
}
