// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use proc_macro2::{Span, TokenStream};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Paren;
use syn::{parenthesized, Ident, LitInt, LitStr, Result, Token, Type, TypeNever, TypeTuple};
use syn::{Attribute, Error};

use quote::{format_ident, quote};

use itertools::MultiUnzip;

pub enum InputParameterSpec {
    Unused(Token![_]),
    Name(Ident),
    Split(Ident),
}

impl InputParameterSpec {
    fn parse_split_attr(input: ParseStream) -> Result<Option<()>> {
        let attributes = input.call(Attribute::parse_outer)?;

        let attr = match attributes.first() {
            Some(attr) => attr,
            None => return Ok(None), // No attribute specified
        };

        let name = attr
            .path
            .get_ident()
            .ok_or_else(|| Error::new(attr.span(), "Empty attribute"))?;
        match name.to_string().as_str() {
            "split" => Ok(Some(())),
            unknown => Err(Error::new(
                name.span(),
                &format!(r#"Unknown attribute "{}", expected "split""#, unknown),
            )),
        }
    }
}

impl Parse for InputParameterSpec {
    fn parse(input: ParseStream) -> Result<Self> {
        let lookahead = input.lookahead1();

        let input_arg = if lookahead.peek(Token![_]) {
            Self::Unused(input.parse()?)
        } else if (Self::parse_split_attr(input)?).is_some() {
            Self::Split(input.parse()?)
        } else {
            Self::Name(input.parse()?)
        };

        Ok(input_arg)
    }
}

struct InputSpec {
    parameters: Punctuated<InputParameterSpec, Token![,]>,
}

impl Parse for InputSpec {
    fn parse(input: ParseStream) -> Result<Self> {
        let input_spec;
        let _in_paren = parenthesized!(input_spec in input);
        let parameters = input_spec.parse_terminated(InputParameterSpec::parse)?;

        Ok(Self { parameters })
    }
}

impl InputSpec {
    fn parameters(&self) -> Vec<InputParameter> {
        let mut parameters = vec![];

        for (param_spec, register) in self.parameters.iter().zip(0usize..) {
            let param = match param_spec {
                InputParameterSpec::Unused(_) => continue,
                InputParameterSpec::Name(ident) => InputParameter::new(ident.clone(), register),
                InputParameterSpec::Split(_) => todo!("Argument splitting not yet implemented"),
            };

            parameters.push(param);
        }

        parameters
    }
}

fn register_name(index: usize, span: Span) -> LitStr {
    LitStr::new(&format!("r{}", index), span)
}

struct InputParameter {
    name: Ident,
    register: usize,
}

impl InputParameter {
    fn new(name: Ident, register: usize) -> Self {
        Self { name, register }
    }

    fn register_spec(&self) -> TokenStream {
        let name = &self.name;
        let reg = register_name(self.register, name.span());

        quote! {
            in(#reg) IntoRegister::into_register(#name)
        }
    }
}

struct OutputParameter {
    ident: Ident,
    ty: Type,
    register: usize,
}

impl OutputParameter {
    fn new(register: usize, ty: Type) -> Self {
        let ident = format_ident!("__out_r{}", register);
        Self {
            ident,
            ty,
            register,
        }
    }

    fn result() -> Self {
        Self::new(0, syn::parse_quote!(u32))
    }

    fn declaration(&self) -> TokenStream {
        let ident = &self.ident;
        quote! {
            let #ident: u32;
        }
    }

    fn register_spec(&self) -> TokenStream {
        let name = &self.ident;
        let reg = register_name(self.register, name.span());

        quote! {
            lateout(#reg) #name
        }
    }

    fn unzip(zipped: Vec<Self>) -> (Vec<Ident>, Vec<Type>, Vec<TokenStream>, Vec<TokenStream>) {
        zipped
            .into_iter()
            .map(|param| {
                let decl = param.declaration();
                let reg = param.register_spec();
                (param.ident, param.ty, decl, reg)
            })
            .multiunzip()
    }
}

pub enum OutputSpec {
    NoReturn(TypeNever),
    Unit,
    Single(Box<Type>),
    Multiple(TypeTuple),
}

impl OutputSpec {
    fn parameters(&self) -> Option<(OutputParameter, Vec<OutputParameter>)> {
        let params = match self {
            Self::NoReturn(_) => return None,
            Self::Unit => vec![],
            Self::Single(ty) => {
                vec![OutputParameter::new(1, (**ty).clone())]
            }
            Self::Multiple(types) => types
                .elems
                .iter()
                .cloned()
                .zip(1usize..)
                .map(|(ty, register_index)| OutputParameter::new(register_index, ty))
                .collect(),
        };

        Some((OutputParameter::result(), params))
    }
}

impl Parse for OutputSpec {
    fn parse(input: ParseStream) -> Result<Self> {
        if input.peek(Token![->]) {
            let _: Token![->] = input.parse()?;
        } else {
            return Ok(Self::Unit);
        }

        let lookahead = input.lookahead1();
        if lookahead.peek(Paren) {
            let types = input.parse()?;
            Ok(Self::Multiple(types))
        } else if lookahead.peek(Token![!]) {
            let never = input.parse()?;
            Ok(Self::NoReturn(never))
        } else {
            let tyype = input.parse()?;
            Ok(Self::Single(tyype))
        }
    }
}

pub struct SvcSpec {
    number: LitInt,
    input: InputSpec,
    output: OutputSpec,
}

impl SvcSpec {
    pub fn to_asm_call(&self) -> Result<TokenStream> {
        let svc_num = self.number.base10_parse::<u8>().map_err(|_| {
            Error::new(
                self.number.span(),
                "SVC number must be in range 0x00..=0xFF",
            )
        })?;
        let svc_mnemonic = LitStr::new(&format!("svc 0x{:02x}", svc_num), self.number.span());

        let inputs = self
            .input
            .parameters()
            .into_iter()
            .map(|p| p.register_spec());

        let asm_call = if let Some((result, output)) = self.output.parameters() {
            let result_code = result.ident.clone();
            let result_decl = result.declaration();
            let result_register = result.register_spec();

            let (output_idents, output_types, output_decl, output_spec) =
                OutputParameter::unzip(output);

            quote! {
                {
                    use crate::result::ResultCode;
                    use crate::svc::{FromRegister, IntoRegister};

                    #result_decl
                    #(#output_decl)*

                    core::arch::asm!(
                        #svc_mnemonic,
                        #(#inputs,)*
                        #result_register,
                        #(#output_spec,)*
                        options(nostack)
                    );

                    ResultCode::from(#result_code).and_then(||
                        (#(<#output_types as FromRegister>::from_register(#output_idents)),*)
                    )
                }
            }
        } else {
            quote! { core::arch::asm!(#svc_mnemonic, #(#inputs,)* options(noreturn, nostack)) }
        };

        Ok(asm_call)
    }
}

impl Parse for SvcSpec {
    fn parse(call_spec: ParseStream) -> Result<Self> {
        let number = call_spec.parse()?;
        let _colon: Token![:] = call_spec.parse()?;

        let input = call_spec.parse()?;
        let output = call_spec.parse()?;

        Ok(Self {
            number,
            input,
            output,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_input_arg() {
        use syn::parse_quote;

        let input_arg: InputParameterSpec = parse_quote!(foo);

        assert!(matches!(input_arg, InputParameterSpec::Name(_)));
    }
}
