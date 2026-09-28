use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream, Result};
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, DeriveInput, Expr, Token};

struct Args {
    exprs: Punctuated<Expr, Token![,]>,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(Args {
            exprs: Punctuated::parse_terminated(input)?,
        })
    }
}

#[proc_macro]
pub fn calvin_storage_group(input: TokenStream) -> TokenStream {
    let name = parse_macro_input!(input as syn::Ident);
    let name_str = name.to_string();

    let expanded = quote! {
        pub struct #name;
        impl #name {
            pub fn get_ring() -> &'static calvin_storage::ring::ShmRing {
                static RING: std::sync::OnceLock<calvin_storage::ring::ShmRing> = std::sync::OnceLock::new();
                RING.get_or_init(|| {
                    let path = format!("/tmp/calvin_ring_{}", #name_str);
                    calvin_storage::ring::ShmRing::open(&path).unwrap_or_else(|_| {
                        calvin_storage::ring::ShmRing::create(&path, 1024 * 1024).unwrap()
                    })
                })
            }
        }
    };

    TokenStream::from(expanded)
}

fn generate_byte_serializers<'a>(
    args: impl Iterator<Item = &'a Expr>,
) -> Vec<proc_macro2::TokenStream> {
    args.map(|arg| {
        quote! {
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    (&#arg as *const _ as *const u8),
                    std::mem::size_of_val(&#arg)
                )
            };
            buffer.extend_from_slice(bytes);
        }
    })
    .collect()
}

fn expand_ring_buffer_push(
    group: &Expr,
    serializers: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    quote! {
        {
            let mut buffer = Vec::new();
            #(#serializers)*
            #group::get_ring().push(&buffer, calvin_storage::ring::QoS::Unreliable);
        }
    }
}

#[proc_macro]
pub fn hstore(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as Args).exprs;
    if args.is_empty() {
        return TokenStream::from(
            quote! { compile_error!("hstore requires at least a group name") },
        );
    }

    let args_vec: Vec<_> = args.into_iter().collect();
    let group = &args_vec[0];
    let serializers = generate_byte_serializers(args_vec.iter().skip(1));
    let expanded = expand_ring_buffer_push(group, &serializers);

    TokenStream::from(expanded)
}

#[proc_macro]
pub fn hlog(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as Args).exprs;
    if args.len() < 2 {
        return TokenStream::from(
            quote! { compile_error!("hlog requires a group name and a format string") },
        );
    }

    let args_vec: Vec<_> = args.into_iter().collect();
    let group = &args_vec[0];
    let serializers = generate_byte_serializers(args_vec.iter().skip(2));
    let expanded = expand_ring_buffer_push(group, &serializers);

    TokenStream::from(expanded)
}

#[proc_macro_derive(CalvinNetClient, attributes(calvin_rpc))]
pub fn derive_calvin_net_client(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    // In a real implementation we would parse the fields and #[calvin_rpc(expr="...")]
    // attributes to build the DEFEXPR and INVOKE RPC stubs.
    let expanded = quote! {
        impl #name {
            pub fn new(host: &str) -> std::io::Result<Self> {
                // Connect and perform HNET_VERSION handshake
                Ok(Self {})
            }
        }
    };

    TokenStream::from(expanded)
}
