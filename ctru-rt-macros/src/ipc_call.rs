use std::marker;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::token::Bracket;
use syn::Token;
use syn::{bracketed, Error, Result};
use syn::{Expr, Ident, LitInt};

struct Parameter {
    expr: Expr,
}

impl Parse for Parameter {
    fn parse(input: ParseStream) -> Result<Self> {
        let expr = input.parse()?;
        Ok(Parameter { expr })
    }
}

struct TranslateParameter;

impl Parse for TranslateParameter {
    fn parse(input: ParseStream) -> Result<Self> {
        let _: Token![_] = input.parse()?;
        Ok(TranslateParameter)
    }
}

struct IpcCall {
    id: u16,
    params_bracket: Bracket,
    params: Punctuated<Parameter, Token![,]>,
    translate_params: Punctuated<TranslateParameter, Token![,]>,
}

impl IpcCall {
    fn header_code(&self) -> Result<u32> {
        let id = u32::from(self.id);

        let num_params = match self.params.len() {
            n if n < (1 << 6) => n as u32,
            _ => {
                return Err(Error::new(
                    self.params_bracket.span,
                    "IPC call has too many normal parameters",
                ))
            }
        };

        Ok((id << 16) | (num_params << 6))
    }

    fn emit_get_tls(&self) -> TokenStream {
        quote! {
            crate::tls::get_thread_local_storage().command_buffer()
        }
    }

    fn emit_buf_write(&self) -> Result<TokenStream> {
        let buffer = Ident::new("__cmdbuf", Span::call_site());
        let command_buffer = self.emit_get_tls();
        let buf_write = IpcBufBuilder::new(buffer.clone(), self.id);

        let writes = buf_write.params(self.params.iter()).build();

        Ok(quote! {
            use ::core::result::Result;

            let #buffer: *mut u32 = #command_buffer;

            #writes

            match crate::svc::send_sync_request((), #buffer) {
                Result::Ok(cmdbuf) => {
                    todo!()
                },
                Result::Err(e) => {
                    Result::Err(e)
                }
            }
        })
    }
}

impl Parse for IpcCall {
    fn parse(input: ParseStream) -> Result<Self> {
        let id = input.parse::<LitInt>()?.base10_parse()?;

        let _colon: Token![:] = input.parse()?;

        let params;
        let params_bracket = bracketed!(params in input);
        let params = params.parse_terminated(Parameter::parse)?;

        let _comma: Token![,] = input.parse()?;

        let translate_params;
        let _translate_params_brackets = bracketed!(translate_params in input);
        let translate_params = translate_params.parse_terminated(TranslateParameter::parse)?;

        Ok(Self {
            id,
            params_bracket,
            params,
            translate_params,
        })
    }
}

mod state {
    pub(crate) struct Header;
    pub(crate) struct NormalParams;
    pub(crate) struct TranslateParameters;
}

struct IpcBufBuilder<S> {
    id: u16,
    buffer: Ident,
    word_offset: usize,
    writes: Vec<TokenStream>,
    normal_params: u32,
    translate_params: u32,
    _state: marker::PhantomData<S>,
}

impl IpcBufBuilder<state::NormalParams> {
    fn new(buffer: Ident, id: u16) -> Self {
        IpcBufBuilder {
            id,
            buffer,
            word_offset: 1,
            writes: Vec::new(),
            normal_params: 0,
            translate_params: 0,
            _state: Default::default(),
        }
    }
}

impl<S> IpcBufBuilder<S> {
    fn transition<T>(self) -> IpcBufBuilder<T> {
        let Self {
            id,
            buffer,
            word_offset,
            writes,
            normal_params,
            translate_params,
            ..
        } = self;
        IpcBufBuilder {
            id,
            buffer,
            word_offset,
            writes,
            normal_params,
            translate_params,
            _state: Default::default(),
        }
    }
}

impl IpcBufBuilder<state::NormalParams> {
    fn params<'p, P: IntoIterator<Item = &'p Parameter>>(
        mut self,
        params: P,
    ) -> IpcBufBuilder<state::TranslateParameters> {
        for p in params.into_iter() {
            self.write_word(&p.expr);
            self.normal_params += 1;
        }
        self.transition()
    }
}

impl<S> IpcBufBuilder<S> {
    fn write_word(&mut self, value: &Expr) {
        let (buffer, offset) = (&self.buffer, self.word_offset);
        let write = quote! {
            #buffer.offset(#offset).write(#value)
        };

        self.word_offset += 1;
        self.writes.push(write);
    }

    fn build(self) -> TokenStream {
        let buffer = self.buffer;
        let writes = self.writes;
        let header =
            (u32::from(self.id) << 16) | (self.normal_params << 6) | (self.translate_params << 0);

        quote! {
            #buffer.write(#header);
            #(#writes;)*
        }
    }
}
