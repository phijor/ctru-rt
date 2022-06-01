// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::{
    parse::{Parse, ParseStream},
    Attribute, Data, DeriveInput, Expr, ExprLit, Fields, Ident, Lit, LitInt, Meta, MetaNameValue,
    NestedMeta, Path, Type, Variant,
};
use syn::{Error, Result};

struct ValuedVariant {
    variant: Variant,
    value: LitInt,
}

pub struct EnumCast {
    ident: Ident,
    variants: Vec<ValuedVariant>,
    value_type: Type,
}

trait PathExt {
    fn is_ident<I>(&self, ident: &I) -> bool
    where
        Ident: PartialEq<I>;
}

impl PathExt for Path {
    fn is_ident<I>(&self, ident: &I) -> bool
    where
        Ident: PartialEq<I>,
    {
        self.get_ident()
            .map(|self_ident| self_ident == ident)
            .unwrap_or(false)
    }
}

impl EnumCast {
    pub fn new(derive_input: DeriveInput) -> Result<Self> {
        if let Data::Enum(enum_input) = derive_input.data {
            let ident = derive_input.ident;
            let mut variants = Vec::with_capacity(enum_input.variants.len());

            let mut next_value = 0;
            for variant in enum_input.variants {
                let variant = Self::parse_variant(variant)?;
                match Self::parse_variant_expr(&variant)? {
                    None => {
                        let span = variant.span();
                        variants.push(ValuedVariant {
                            variant,
                            value: LitInt::new(&format!("{}", next_value), span),
                        });
                    }
                    Some(value) => {
                        next_value = value.base10_parse()?;
                        variants.push(ValuedVariant { variant, value });
                    }
                };

                next_value += 1;
            }

            let value_type = Self::parse_value_type(&derive_input.attrs)?;

            Ok(Self {
                ident,
                variants,
                value_type,
            })
        } else {
            Err(Error::new(
                derive_input.span(),
                "EnumCast can only be used on enums",
            ))
        }
    }

    fn parse_value_type(attributes: &[Attribute]) -> Result<Type> {
        for attr in attributes {
            let meta = match attr.parse_meta() {
                Ok(Meta::List(meta)) => meta,
                _ => continue,
            };

            if meta.path.is_ident("enum_cast") {
                for nested in meta.nested {
                    match nested {
                        NestedMeta::Meta(Meta::NameValue(MetaNameValue {
                            path,
                            lit: Lit::Str(type_lit),
                            ..
                        })) if path.is_ident("value_type") => return type_lit.parse(),
                        _ => continue,
                    }
                }
            }
        }

        Ok(syn::parse_quote!(u32))
    }

    fn parse_variant(variant: Variant) -> Result<Variant> {
        if let Fields::Unit = variant.fields {
            Ok(variant)
        } else {
            Err(Error::new(
                variant.fields.span(),
                "EnumCast can only be used on enums with field-less variants",
            ))
        }
    }

    fn parse_variant_expr(variant: &Variant) -> Result<Option<LitInt>> {
        if let Some((_, expr)) = &variant.discriminant {
            if let Expr::Lit(ExprLit {
                lit: Lit::Int(lit), ..
            }) = expr
            {
                Ok(Some(lit.clone()))
            } else {
                Err(Error::new(
                    expr.span(),
                    "EnumCast variant can only be assigned integer literals",
                ))
            }
        } else {
            Ok(None)
        }
    }

    fn emit_from_value(&self) -> TokenStream {
        let value_type = &self.value_type;

        let (variant_idents, values): (Vec<&Ident>, Vec<&LitInt>) = self
            .variants
            .iter()
            .map(|v| (&v.variant.ident, &v.value))
            .unzip();

        let result_type: Path = syn::parse_quote!(::core::result::Result);

        quote! {
            pub const fn from_value(value: #value_type) -> #result_type<Self, #value_type> {
                match value {
                    #(#values => #result_type::Ok(Self::#variant_idents),)*
                    unmatched => #result_type::Err(unmatched),
                }
            }
        }
    }

    fn emit_to_value(&self) -> TokenStream {
        let value_type = &self.value_type;

        let (variant_idents, values): (Vec<&Ident>, Vec<&LitInt>) = self
            .variants
            .iter()
            .map(|v| (&v.variant.ident, &v.value))
            .unzip();

        quote! {
            pub const fn to_value(&self) -> #value_type {
                match self {
                    #(Self::#variant_idents => #values,)*
                }
            }
        }
    }

    pub fn emit(&self) -> TokenStream {
        let ident = &self.ident;
        let from_value = self.emit_from_value();
        let to_value = self.emit_to_value();

        quote! {
            impl #ident {
                #from_value

                #to_value
            }
        }
    }
}

impl Parse for EnumCast {
    fn parse(input: ParseStream) -> Result<Self> {
        let input = DeriveInput::parse(input)?;
        Self::new(input)
    }
}
