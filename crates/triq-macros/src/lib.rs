//! `#[derive(Ports)]` turns a struct of `Input<T>` and `Output<T>` fields into a
//! port declaration. The field name is the port name; `#[port(...)]` sets options.
//!
//! `#[derive(Fields)]` makes a message type list the numbers it holds: each
//! member's own numbers, by the member's name and where it begins.
#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Member, parse_macro_input};

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

#[proc_macro_derive(Fields)]
pub fn derive_fields(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_fields(&input).unwrap_or_else(|error| error.to_compile_error()).into()
}

fn expand_fields(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let ident = &input.ident;
    let Data::Struct(data) = &input.data else {
        let instead = format!("Fields can only be derived for a struct; write `impl triq::Fields for {ident} {{}}`");
        return Err(syn::Error::new_spanned(ident, instead));
    };
    let members = data.fields.iter().zip(data.fields.members()).map(|(field, member)| {
        let ty = &field.ty;
        // A member of a tuple struct is called by its place: `0`, `1`.
        let word = match &member {
            Member::Named(ident) => ident.to_string().trim_start_matches("r#").to_owned(),
            Member::Unnamed(index) => index.index.to_string(),
        };
        quote! {
            <#ty as ::triq::Fields>::fields(
                &::triq::member(name, #word),
                at + ::core::mem::offset_of!(Self, #member),
                found,
            );
        }
    });
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::triq::Fields for #ident #ty_generics #where_clause {
            fn fields(name: &str, at: usize, found: &mut ::std::vec::Vec<::triq::Field>) {
                #(#members)*
            }
        }
    })
}
