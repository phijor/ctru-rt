// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![allow(dead_code)]

mod entry;
mod enum_cast;
mod ipc_call;
mod svc_spec;

use crate::enum_cast::EnumCast;
use crate::svc_spec::SvcSpec;

use syn::{parse_macro_input, AttributeArgs, ItemFn};

// use proc_macro2::{Span, TokenStream};
// use proc_macro_hack::proc_macro_hack;
// use quote::{quote, ToTokens};
// use syn::{
//     parse::{Parse, ParseStream, Result},
//     parse_macro_input,
//     spanned::Spanned,
//     Attribute, Error, Expr, FnArg, Ident, ItemFn, Lit, LitInt, Pat, PatIdent, PatType, Signature,
//     Token, Type, TypeArray, TypeTuple,
// };
//
// use core::convert::TryFrom;

// fn parse_len(expr: Expr) -> Result<usize> {
//     match expr {
//         Expr::Lit(expr) => match expr.lit {
//             Lit::Int(lit) => lit.base10_parse(),
//             _ => Err(Error::new(
//                 expr.span(),
//                 "Length expressions must be integer literals",
//             )),
//         },
//         _ => Err(Error::new(
//             expr.span(),
//             "Length expressions must be integer literals",
//         )),
//     }
// }
//
// #[derive(Debug, Copy, Clone, PartialEq)]
// enum HandleType {
//     Copy,
//     Move,
// }
//
// struct HandleList {}
//
// #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
// enum ArgumentType {
//     Parameter,
//     Handle,
//     Pid,
// }
//
// struct Parameters {
//     ident: Ident,
//     types: TypeTuple,
// }
//
// struct Handles {
//     ident: Ident,
//     ty: Type,
//     len: usize,
// }
//
// enum Argument {
//     Parameters(Parameters),
//     Handles(Handles),
//     Pid,
// }
//
// impl Argument {
//     fn from_fn_argument(fn_arg: FnArg) -> Result<Self> {
//         let argument = match fn_arg {
//             FnArg::Receiver(argument) => {
//                 return Err(Error::new(
//                     argument.span(),
//                     "IPC request prototype may not have a receiver input",
//                 ))
//             }
//             FnArg::Typed(argument) => argument,
//         };
//
//         let ident = match *argument.pat {
//             Pat::Ident(pat) => pat.ident,
//             _ => {
//                 return Err(Error::new(
//                     argument.pat.span(),
//                     "IPC request argument must be an identifier, not a pattern",
//                 ))
//             }
//         };
//
//         enum ArgumentTypeTag {
//             Parameter,
//             Handles,
//             Pid,
//         }
//
//         if let Some(attr) = argument.attrs.first() {
//             match attr.path.get_ident().map(|i| i.to_string()).as_deref() {
//                 Some("handles") => match *argument.ty {
//                     Type::Array(ty) => Ok(Self::Handles(Handles {
//                         ident,
//                         ty: *ty.elem,
//                         len: parse_len(ty.len)?,
//                     })),
//                     _ => Err(Error::new(
//                         argument.ty.span(),
//                         "IPC handle parameters must be passed as a fixed size array",
//                     )),
//                 },
//                 Some("pid") => Ok(Self::Pid),
//                 _ => Err(Error::new(
//                     ident.span(),
//                     "IPC request argument may only be marked as `#[handle]` or `#[pid]`",
//                 )),
//             }
//         } else {
//             match *argument.ty {
//                 Type::Tuple(ty) => Ok(Self::Parameters(Parameters { ident, types: ty })),
//                 _ => Err(Error::new(
//                     argument.ty.span(),
//                     "IPC parameters must be passed as a tuple",
//                 )),
//             }
//         };
//     }
// }
//
// impl ToTokens for Argument {
//     fn to_tokens(&self, output: &mut TokenStream) {
//         unimplemented!()
//         // self.fn_arg.to_tokens(output)
//     }
// }
//
// struct RequestFn {
//     function: ItemFn,
//     parameters: Vec<Argument>,
//     translate_parameters: Vec<Argument>,
// }
//
// impl Parse for RequestFn {
//     fn parse(input: ParseStream) -> Result<Self> {
//         let function = input.parse::<ItemFn>()?;
//
//         let signature = function.sig;
//
//         if signature.constness.is_some()
//             || signature.asyncness.is_some()
//             || signature.abi.is_some()
//             // || !signature.generics.params.is_empty()
//             // || signature.generics.where_clause.is_some()
//             || signature.variadic.is_some()
//         {
//             return Err(Error::new(
//                 signature.span(),
//                 "IPC request prototype has to be in the form of `fn request(...) -> (...)`",
//             ));
//         }
//
//         let arguments = signature.inputs.into_iter();
//         let parameters = arguments.next().ok_or_else(|| Error::new(signature.span(), "IPC request needs parameters")).and_then(|fn_arg| )
//
//         let mut handles = Vec::new();
//         let current_type = ArgumentType::Parameter;
//
//         for argument in arguments {
//             match argument.tag {
//                 ArgumentType::Parameter => {
//                     parameters.push(argument);
//                 }
//                 ArgumentType::Handle => {}
//             }
//         }
//
//         let ident = signature.ident;
//         let generics = signature.generics;
//
//         Ok(Self {
//             arguments,
//             function,
//         })
//     }
// }
//
// impl RequestFn {
//     fn into_fn(&self) -> TokenStream {
//         let ident = self.function.sig.ident;
//         let generics = self.function.sig.generics;
//         let generic_params = generics.params;
//         let where_clause = generics.where_clause;
//         let arguments = self.arguments;
//
//         quote! {
//             unsafe fn #ident #generic_params (buffer: *mut u32, #(#arguments),* ) -> ()
//                 #where_clause
//             {
//             }
//         }
//     }
//
//     fn build_body(&self) -> TokenStream {
//         let mut statements = Vec::new();
//         let mut current_type = None;
//         let mut offset: u16 = 1;
//
//         for argument in self.arguments {
//             match argument.tag {
//                 ArgumentType::Parameter => {}
//                 _ => unimplemented!(),
//             }
//         }
//     }
// }
//
// /// Fill out a IPC buffer
// ///
// /// #[ipc_request(0x42)]
// /// fn register_client<'a>(foo: u32, #[handles] blah: [WeakHandle<'a>; 1], #[pid] _: ()) -> () {}
// #[proc_macro_attribute]
// pub fn ipc_request(
//     attr: proc_macro::TokenStream,
//     item: proc_macro::TokenStream,
// ) -> proc_macro::TokenStream {
//     let request = parse_macro_input!(item as Request);
//
//     request.build().into()
// }

#[proc_macro]
pub fn svc(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let call_spec = parse_macro_input!(tokens as SvcSpec);

    let output: proc_macro2::TokenStream = call_spec.to_asm_call();

    output.into()
}

#[proc_macro_derive(EnumCast, attributes(enum_cast))]
pub fn enum_cast_impl(tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let enum_cast = parse_macro_input!(tokens as EnumCast);
    enum_cast.emit().into()
}

#[proc_macro_attribute]
pub fn entry(
    args: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let args = parse_macro_input!(args as AttributeArgs);
    let f = parse_macro_input!(item as ItemFn);

    entry::entry(args, f).into()
}
