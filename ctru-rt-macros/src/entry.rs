use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{parse, AttributeArgs, ItemFn, ReturnType, Type, Visibility};

pub(crate) fn entry(args: AttributeArgs, entry_point: ItemFn) -> TokenStream {
    let sig = &entry_point.sig;

    let vis_inherited = match entry_point.vis {
        Visibility::Inherited => true,
        _ => false,
    };

    let valid_return_type = match sig.output {
        ReturnType::Default => true,
        ReturnType::Type(_, ref ty) => match **ty {
            Type::Tuple(ref tup) => tup.elems.is_empty(),
            Type::Never(_) => true,
            _ => false,
        },
    };

    let valid_signature = sig.constness.is_none()
        && vis_inherited
        && sig.abi.is_none()
        && sig.inputs.is_empty()
        && sig.generics.params.is_empty()
        && sig.generics.where_clause.is_none()
        && sig.variadic.is_none()
        && valid_return_type;

    if !valid_signature {
        return parse::Error::new_spanned(
            sig,
            "`#[entry]` function must have signature `[unsafe] fn()` or `[unsafe] fn() -> !`",
        )
        .to_compile_error()
        .into();
    }

    if !args.is_empty() {
        return parse::Error::new(Span::call_site(), "This attribute accepts no arguments")
            .to_compile_error()
            .into();
    }

    let ident = &entry_point.sig.ident;

    quote! {
        #[inline(always)]
        #entry_point

        #[export_name = "_ctru_rt_entry"]
        pub unsafe fn _ctru_rt_entry() {
            #ident()
        }
    }
}
