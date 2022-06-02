// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Paren;
use syn::{parenthesized, Ident, LitInt, LitStr, Result, Token, Type, TypeNever, TypeTuple};
use syn::{Attribute, Error};

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
    pub(crate) parameters: Punctuated<InputParameterSpec, Token![,]>,
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
    svc_num: u8,
    input: InputSpec,
    output: OutputSpec,
}

impl SvcSpec {
    pub fn to_asm_call(&self) -> TokenStream {
        let svc_mnemonic = LitStr::new(&format!("svc 0x{:02x}", self.svc_num), self.svc_num.span());

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

        asm_call
    }
}

impl Parse for SvcSpec {
    fn parse(call_spec: ParseStream) -> Result<Self> {
        let svc_num_lit: LitInt = call_spec.parse()?;

        let svc_num = svc_num_lit.base10_parse::<u8>().map_err(|_| {
            Error::new(
                svc_num_lit.span(),
                "SVC number must be in range 0x00..=0xFF",
            )
        })?;

        let _colon: Token![:] = call_spec.parse()?;

        let input = call_spec.parse()?;
        let output = call_spec.parse()?;

        Ok(Self {
            svc_num,
            input,
            output,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use assert_matches::assert_matches;
    use syn::parse_quote;

    use std::fmt::{self, Debug};

    impl Debug for InputParameterSpec {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Unused(_) => f.debug_tuple("Unused").field(&"_").finish(),
                Self::Name(ident) => f.debug_tuple("Name").field(ident).finish(),
                Self::Split(_) => f.debug_tuple("Split").field(&"_").finish(),
            }
        }
    }

    impl Debug for OutputSpec {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::NoReturn(_) => f.debug_tuple("NoReturn").field(&"_").finish(),
                Self::Unit => write!(f, "Unit"),
                Self::Single(_) => f.debug_tuple("Single").field(&"_").finish(),
                Self::Multiple(tuple) => f
                    .debug_tuple("Multiple")
                    .field(&format!("[_; {}]", tuple.elems.len()))
                    .finish(),
            }
        }
    }

    macro_rules! test_spec {
        ($($name:ident: [ $($spec:tt)* ] => $expected:pat $(if $cond:expr)?),*$(,)?) => {
            $(
                #[test]
                fn $name() {
                    let spec = parse_quote!{ $($spec)* };

                    assert_matches!(spec, $expected $(if $cond)?);
                }
            )*
        };
    }

    test_spec! {
        parse_input_param_named:
            [foo] => InputParameterSpec::Name(ident) if ident == "foo",
        parse_input_param_unused:
            [_] => InputParameterSpec::Unused(_),
        parse_input_param_split:
            [#[split] bar] => InputParameterSpec::Split(ident) if ident == "bar",
    }

    #[test]
    fn parse_input_spec() {
        let spec: InputSpec = parse_quote! { (foo, _, #[split] bar) };

        let params: [_; 3] = spec
            .parameters
            .into_iter()
            .collect::<Vec<_>>()
            .try_into()
            .expect("Expected to parse 3 parameters");

        use InputParameterSpec::*;

        assert_matches!(params, [Name(named), Unused(_), Split(split)] if named == "foo" && split == "bar");
    }

    test_spec! {
        parse_output_spec_unit:
            [] => OutputSpec::Unit,
        parse_output_spec_no_return:
            [-> !] => OutputSpec::NoReturn(_),
        parse_output_spec_single:
            [-> u32] => OutputSpec::Single(_),
        parse_output_spec_multiple:
            [-> (u32, u32)] => OutputSpec::Multiple(tuple) if tuple.elems.len() == 2,
        parse_output_spec_multiple_empty:
            [-> ()] => OutputSpec::Multiple(tuple) if tuple.elems.is_empty(),
    }
}
