// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use darling::FromMeta;
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Paren;
use syn::{
    parenthesized, Ident, LitInt, LitStr, NestedMeta, Result, Token, Type, TypeNever, TypeTuple,
};
use syn::{Attribute, Error};

use itertools::MultiUnzip;

#[derive(PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
struct Register(usize);

impl FromMeta for Register {
    fn from_string(value: &str) -> darling::Result<Self> {
        let err_invalid_reg_name = || {
            darling::Error::custom(&format!(
                r#"expected register name "r{{0..15}}", not "{value}""#
            ))
        };

        let register_index = value.strip_prefix('r').ok_or_else(err_invalid_reg_name)?;

        let index = register_index.parse().map_err(|_| err_invalid_reg_name())?;

        Ok(Self(index))
    }
}

#[derive(Default, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub struct SplitAttribute {
    low: Option<Register>,
    high: Option<Register>,
}

impl FromMeta for SplitAttribute {
    fn from_word() -> darling::Result<Self> {
        Ok(Self::default())
    }

    fn from_list(items: &[NestedMeta]) -> darling::Result<Self> {
        let mut split_attr = Self::default();

        let err_unexpected = Err(darling::Error::custom(
            r#"Expected attributes "low = {{reg}}" or "high = {{reg}}""#,
        ));

        for item in items {
            match item {
                NestedMeta::Meta(meta) => {
                    let path = meta.path();

                    if path.is_ident("low") {
                        split_attr.low = Some(Register::from_meta(meta)?);
                    } else if path.is_ident("high") {
                        split_attr.high = Some(Register::from_meta(meta)?);
                    } else {
                        return err_unexpected;
                    }
                }
                NestedMeta::Lit(_) => return err_unexpected,
            }
        }

        Ok(split_attr)
    }
}

#[cfg_attr(test, derive(Debug))]
pub enum InputParameterSpec {
    Unused(Token![_]),
    Name(Ident),
    Split(SplitAttribute, Ident),
}

impl InputParameterSpec {
    fn parse_split_attr(input: ParseStream) -> Result<Option<SplitAttribute>> {
        for attr in input.call(Attribute::parse_outer)? {
            if attr.path.is_ident("split") {
                let meta = attr.parse_meta()?;
                let split_attr = SplitAttribute::from_meta(&meta)?;

                return Ok(Some(split_attr));
            }
        }

        Ok(None)
    }
}

impl Parse for InputParameterSpec {
    fn parse(input: ParseStream) -> Result<Self> {
        let lookahead = input.lookahead1();

        let input_arg = if lookahead.peek(Token![_]) {
            Self::Unused(input.parse()?)
        } else if let Some(attr) = Self::parse_split_attr(input)? {
            Self::Split(attr, input.parse()?)
        } else {
            Self::Name(input.parse()?)
        };

        Ok(input_arg)
    }
}

#[cfg_attr(test, derive(Debug))]
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
                InputParameterSpec::Split(_, _) => todo!("Argument splitting not yet implemented"),
            };

            parameters.push(param);
        }

        parameters
    }

    #[cfg(test)]
    fn into_param_array<const N: usize>(self) -> Option<[InputParameterSpec; N]> {
        self.parameters
            .into_iter()
            .collect::<Vec<_>>()
            .try_into()
            .ok()
    }

    fn emit_register_specs(&self) -> impl Iterator<Item = TokenStream> {
        self.parameters()
            .into_iter()
            .map(|p| p.emit_register_spec())
    }
}

fn register_name(index: usize, span: Span) -> LitStr {
    LitStr::new(&format!("r{}", index), span)
}

#[cfg_attr(test, derive(Debug))]
struct InputParameter {
    name: Ident,
    register: usize,
}

impl InputParameter {
    fn new(name: Ident, register: usize) -> Self {
        Self { name, register }
    }

    fn emit_register_spec(&self) -> TokenStream {
        let name = &self.name;
        let reg = register_name(self.register, name.span());

        quote! {
            in(#reg) IntoRegister::into_register(#name)
        }
    }
}

#[cfg_attr(test, derive(Debug))]
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

#[cfg_attr(test, derive(Debug))]
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

#[cfg_attr(test, derive(Debug))]
pub struct SvcSpec {
    svc_num: u8,
    input: InputSpec,
    output: OutputSpec,
}

impl SvcSpec {
    fn emit_svc_mnemonic(&self) -> LitStr {
        LitStr::new(&format!("svc 0x{:02x}", self.svc_num), self.svc_num.span())
    }
    pub fn to_asm_call(&self) -> TokenStream {
        let svc_mnemonic = self.emit_svc_mnemonic();

        let input_specs = self.input.emit_register_specs();

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
                        #(#input_specs,)*
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
            quote! { core::arch::asm!(#svc_mnemonic, #(#input_specs,)* options(noreturn, nostack)) }
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
            [#[split] bar] => InputParameterSpec::Split(split, ident)
                if split == Default::default() && ident == "bar",
        parse_input_param_split_registers:
            [#[split(low = "r0", high = "r4")] bar] => InputParameterSpec::Split(
                SplitAttribute { low: Some(Register(0)), high: Some(Register(4)) },
                _
            ),
    }

    #[test]
    fn parse_input_param_split_unknown_attr() {
        let res = syn::parse2::<InputParameterSpec>(quote!(#[split(unknown = "...") foo]));

        assert_matches!(res, Err(_));
    }

    #[test]
    fn parse_input_spec() {
        let spec: InputSpec = parse_quote! { (foo, _, #[split] bar) };

        let params: [_; 3] = spec
            .into_param_array()
            .expect("Expected to parse 3 parameters");

        use InputParameterSpec::*;

        assert_matches!(
            params,
            [
                Name(named),
                Unused(_),
                Split(SplitAttribute { low: None, high: None }, split),
            ] if named == "foo" && split == "bar"
        );
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

    #[test]
    fn parse_svc_spec() {
        use InputParameterSpec::*;

        let SvcSpec {
            svc_num,
            input,
            output,
        } = parse_quote! { 0xff: (foo, _) -> u32 };

        let input_params = input
            .into_param_array()
            .expect("Expected exactly one named and one unnamed input parameter");

        assert_eq!(svc_num, 0xff);
        assert_matches!(input_params, [Name(named), Unused(_)] if named == "foo");
        assert_matches!(output, OutputSpec::Single(_));
    }

    #[test]
    fn parse_svc_spec_invalid_svc_number() {
        let res: Result<SvcSpec> = syn::parse2(quote!(0x100: ()));

        assert_matches!(res, Err(_));
    }

    #[test]
    fn emit_svc_mnemonic() {
        let spec: SvcSpec = parse_quote!(0xff: ());

        let expected: LitStr = parse_quote! { "svc 0xff" };

        assert_eq!(spec.emit_svc_mnemonic(), expected);
    }

    #[test]
    fn input_spec_to_parameters() {
        let spec: InputSpec = parse_quote! { (foo, _, bar) };

        let [foo, bar]: [InputParameter; 2] = spec
            .parameters()
            .try_into()
            .expect("Expected two parameters, skipping one of three in the spec");

        assert_eq!(foo.name, "foo");
        assert_eq!(foo.register, 0);
        // ... skipping r1 ...
        assert_eq!(bar.name, "bar");
        assert_eq!(bar.register, 2);
    }
}
