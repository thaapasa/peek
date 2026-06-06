//! `#[derive(InfoSection)]` — generate the print half of an info section from
//! the same view struct that `serde::Serialize` drives for JSON.
//!
//! The generated `impl InfoSection` walks the struct's named fields in
//! declaration order, emitting one `(label, painted_value)` row per visible
//! field. Each row's *label* comes from `#[info(label = "…")]`; its *value*
//! from the field type's `InfoValue` impl. A field is skipped when its skip
//! predicate holds — resolved (highest precedence first) from
//! `#[info(skip_if_zero)]`, `#[info(skip_if = "path")]`, or the field's own
//! `#[serde(skip_serializing_if = "path")]`, so a skip declared once for JSON
//! is mirrored in print unless print needs to diverge.
//!
//! The struct attribute `#[info(title = "…")]` supplies the section header.
//!
//! Generated paths are fully qualified through `::peek_foundation` — the one
//! crate a call site is guaranteed to have in scope (it's where the derive is
//! re-exported from). Even `PeekTheme` is reached via `::peek_foundation::theme`
//! rather than `::peek_theme`, so the macro never assumes a second dependency.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, parse_macro_input};

/// Per-field skip predicate, in precedence order.
enum Skip {
    /// `#[info(skip_if_zero)]` — hide when the value is numerically zero.
    Zero,
    /// `#[info(skip_if = "path")]` or `#[serde(skip_serializing_if = "path")]`
    /// — hide when `path(&field)` is true.
    Pred(syn::Path),
    /// Always rendered.
    None,
}

#[proc_macro_derive(InfoSection, attributes(info))]
pub fn derive_info_section(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;

    let title = struct_title(input)?;

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    name,
                    "InfoSection requires a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "InfoSection can only be derived for structs",
            ));
        }
    };

    let mut row_stmts = Vec::new();
    for field in fields {
        let ident = field.ident.as_ref().expect("named field");
        let label = field_label(field)?;
        let skip = field_skip(field)?;

        let push = quote! {
            rows.push((
                #label,
                ::peek_foundation::info::InfoValue::render_value(&self.#ident, theme),
            ));
        };

        let stmt = match skip {
            Skip::None => push,
            Skip::Zero => quote! {
                if !::peek_foundation::info::MaybeZero::is_zero_value(&self.#ident) {
                    #push
                }
            },
            Skip::Pred(path) => quote! {
                if !#path(&self.#ident) {
                    #push
                }
            },
        };
        row_stmts.push(stmt);
    }

    Ok(quote! {
        impl ::peek_foundation::info::InfoSection for #name {
            fn title(&self) -> &'static str {
                #title
            }

            fn rows(
                &self,
                theme: &::peek_foundation::theme::PeekTheme,
            ) -> ::std::vec::Vec<(&'static str, ::std::string::String)> {
                let mut rows = ::std::vec::Vec::new();
                #(#row_stmts)*
                rows
            }
        }
    })
}

/// Pull the required `#[info(title = "…")]` off the struct.
fn struct_title(input: &DeriveInput) -> syn::Result<String> {
    let mut title = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("info") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("title") {
                let s: LitStr = meta.value()?.parse()?;
                title = Some(s.value());
                Ok(())
            } else {
                Err(meta.error("unknown `info` struct attribute (expected `title`)"))
            }
        })?;
    }
    title.ok_or_else(|| {
        syn::Error::new_spanned(
            &input.ident,
            "missing `#[info(title = \"…\")]` on the struct",
        )
    })
}

/// Pull the required `#[info(label = "…")]` off a field.
fn field_label(field: &syn::Field) -> syn::Result<String> {
    let mut label = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("info") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("label") {
                let s: LitStr = meta.value()?.parse()?;
                label = Some(s.value());
            }
            // Other `info` keys (skip_if_zero / skip_if) are read separately;
            // consume any value they carry so parsing doesn't trip.
            else if meta.input.peek(syn::Token![=]) {
                let _: syn::Expr = meta.value()?.parse()?;
            }
            Ok(())
        })?;
    }
    label.ok_or_else(|| {
        syn::Error::new_spanned(field, "missing `#[info(label = \"…\")]` on the field")
    })
}

/// Resolve a field's skip predicate: `info(skip_if_zero)` >
/// `info(skip_if = "path")` > `serde(skip_serializing_if = "path")` > none.
fn field_skip(field: &syn::Field) -> syn::Result<Skip> {
    let mut zero = false;
    let mut info_pred: Option<syn::Path> = None;

    for attr in &field.attrs {
        if !attr.path().is_ident("info") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip_if_zero") {
                zero = true;
            } else if meta.path.is_ident("skip_if") {
                let s: LitStr = meta.value()?.parse()?;
                info_pred = Some(s.parse()?);
            } else if meta.input.peek(syn::Token![=]) {
                // label = "…" (read elsewhere) or any other valued key.
                let _: syn::Expr = meta.value()?.parse()?;
            }
            Ok(())
        })?;
    }

    if zero {
        return Ok(Skip::Zero);
    }
    if let Some(p) = info_pred {
        return Ok(Skip::Pred(p));
    }
    if let Some(p) = serde_skip_if(field)? {
        return Ok(Skip::Pred(p));
    }
    Ok(Skip::None)
}

/// Read `#[serde(skip_serializing_if = "path")]` off a field, ignoring every
/// other serde key (`rename`, `default`, `flatten`, …) — those affect JSON
/// only, not the print label.
fn serde_skip_if(field: &syn::Field) -> syn::Result<Option<syn::Path>> {
    let mut pred = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip_serializing_if") {
                let s: LitStr = meta.value()?.parse()?;
                pred = Some(s.parse()?);
            } else if meta.input.peek(syn::Token![=]) {
                let _: syn::Expr = meta.value()?.parse()?;
            }
            // Flag-only serde keys (default, flatten) carry no value: nothing
            // to consume, just continue.
            Ok(())
        })?;
    }
    Ok(pred)
}
