//! `#[derive(Ports)]` turns a struct of `Input<T>` and `Output<T>` fields into a
//! port declaration. The field name is the port name; `#[port(...)]` sets options.
use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

const OPTIONS: [&str; 5] = ["queue", "subscribers", "hold", "reliable_ms", "depth"];

#[proc_macro_derive(Ports, attributes(port))]
pub fn derive_ports(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input).unwrap_or_else(|error| error.to_compile_error()).into()
}

fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(&input.ident, "Ports needs named fields"));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(&input.ident, "Ports can only be derived for a struct"));
        }
    };
    let mut specs = Vec::new();
    let mut binds = Vec::new();
    for field in fields {
        let ident = field.ident.as_ref().expect("named field");
        let name = ident.to_string().trim_start_matches("r#").to_owned();
        let ty = &field.ty;
        let options = options(field)?;
        specs.push(quote!(<#ty as ::triq::PortField>::spec(#name, #options)));
        binds.push(quote!(#ident: <#ty as ::triq::PortField>::bind(binder, #name, #options)?));
    }
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::triq::Ports for #ident #ty_generics #where_clause {
            fn spec() -> ::std::vec::Vec<::triq::PortSpec> {
                ::std::vec![#(#specs),*]
            }
            fn bind(binder: &mut ::triq::Binder) -> ::triq::Result<Self> {
                ::std::result::Result::Ok(Self { #(#binds),* })
            }
        }
    })
}

/// `#[port(queue = 4, depth = 1)]` becomes `PortOptions { queue: Some(4), depth: Some(1), .. }`.
fn options(field: &syn::Field) -> syn::Result<proc_macro2::TokenStream> {
    let mut settings = Vec::new();
    for attribute in field.attrs.iter().filter(|attribute| attribute.path().is_ident("port")) {
        attribute.parse_nested_meta(|meta| {
            let key = meta.path.get_ident().cloned();
            let Some(key) = key.filter(|key| OPTIONS.contains(&key.to_string().as_str())) else {
                return Err(meta.error(format!("unknown port option; expected one of {}", OPTIONS.join(", "))));
            };
            let value: syn::LitInt = meta.value()?.parse()?;
            settings.push(quote!(#key: ::std::option::Option::Some(#value)));
            Ok(())
        })?;
    }
    Ok(quote!(::triq::PortOptions { #(#settings,)* ..::std::default::Default::default() }))
}
