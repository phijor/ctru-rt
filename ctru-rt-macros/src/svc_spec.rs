use proc_macro2::{Span, TokenStream};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::token::Paren;
use syn::{parenthesized, LitStr, Result, Type, TypeNever, TypeTuple};
use syn::{Ident, LitInt, Token};

use quote::{format_ident, quote};

pub enum InputArg {
    Unused(Token![_]),
    Name(Ident),
}

impl Parse for InputArg {
    fn parse(input: ParseStream) -> Result<Self> {
        let lookahead = input.lookahead1();

        if lookahead.peek(Token![_]) {
            Ok(Self::Unused(input.parse()?))
        } else {
            Ok(Self::Name(input.parse()?))
        }
    }
}

pub enum OutputSpec {
    NoReturn(TypeNever),
    Single(Type),
    Multiple(TypeTuple),
}

impl OutputSpec {
    fn declarations(&self) -> Vec<TokenStream> {
        self.idents()
            .iter()
            .map(|ident| {
                quote! {
                    let #ident: u32;
                }
            })
            .collect()
    }

    fn types(&self) -> Vec<Type> {
        match self {
            Self::NoReturn(never) => vec![Type::from(never.clone())],
            Self::Single(ty) => vec![ty.clone()],
            Self::Multiple(types) => types.elems.iter().cloned().collect(),
        }
    }

    fn idents(&self) -> Vec<Ident> {
        match self {
            Self::NoReturn(_) => vec![],
            Self::Single(_) => vec![format_ident!("__out")],
            Self::Multiple(types) => types
                .elems
                .iter()
                .zip(1usize..)
                .map(|(_type, register_index)| format_ident!("__out_r{}", register_index))
                .collect(),
        }
    }
}

impl Parse for OutputSpec {
    fn parse(input: ParseStream) -> Result<Self> {
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
    colon: Token![:],
    in_paren: Paren,
    input: Punctuated<InputArg, Token![,]>,
    arrow: Token![->],
    output: OutputSpec,
}

impl SvcSpec {
    fn register_name(index: usize, span: Span) -> LitStr {
        LitStr::new(&format!("r{}", index), span)
    }

    fn input_arg(register_index: usize, name: &Ident) -> TokenStream {
        let reg = SvcSpec::register_name(register_index, name.span());

        quote! {
            in(#reg) IntoRegister::into_register(#name)
        }
    }

    fn output_arg(register_index: usize, name: &Ident) -> TokenStream {
        let reg = SvcSpec::register_name(register_index, name.span());

        quote! {
            lateout(#reg) #name
        }
    }

    pub fn to_asm_call(&self) -> Result<TokenStream> {
        let svc_mnemonic = LitStr::new(
            &format!("svc {}", self.number.base10_parse::<u8>()?),
            self.number.span(),
        );

        let inputs = self
            .input
            .iter()
            .enumerate()
            .filter_map(|(register_index, arg)| match arg {
                InputArg::Unused(_) => None,
                InputArg::Name(ident) => Some(Self::input_arg(register_index, ident)),
            });

        let asm_call = match self.output {
            OutputSpec::NoReturn(_) => {
                quote! { asm!(#svc_mnemonic, #(#inputs,)* options(noreturn, nostack)) }
            }
            _ => {
                let result_code = Ident::new("__out_r0_result_code", Span::call_site());
                let result_register = Self::output_arg(0, &result_code);

                let output_idents = self.output.idents();
                let output_types = self.output.types();
                let output_spec =
                    (1usize..)
                        .zip(output_idents.iter())
                        .map(|(register_index, output_ident)| {
                            Self::output_arg(register_index, output_ident)
                        });
                let output_decl = self.output.declarations();

                quote! {
                    {
                        use crate::result::ResultCode;
                        use crate::svc::{FromRegister, IntoRegister};

                        let #result_code: u32;

                        #(#output_decl)*

                        asm!(
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
            }
        };

        Ok(asm_call)
    }
}

impl Parse for SvcSpec {
    fn parse(call_spec: ParseStream) -> Result<Self> {
        let number = call_spec.parse()?;
        let colon = call_spec.parse()?;

        let input;
        let in_paren = parenthesized!(input in call_spec);
        let input = input.parse_terminated(InputArg::parse)?;

        let arrow = call_spec.parse()?;

        let output = call_spec.parse()?;

        Ok(Self {
            number,
            colon,
            in_paren,
            input,
            arrow,
            output,
        })
    }
}
