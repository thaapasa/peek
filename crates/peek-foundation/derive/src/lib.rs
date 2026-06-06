//! `#[derive(InfoView)]` — generate the print half of a type's info view from
//! the same struct that `serde::Serialize` drives for JSON.
//!
//! The generated `info_nodes()` walks the struct's named fields in declaration
//! order, building a tree of `InfoNode`s:
//!
//! - a **scalar** field (`#[info(label = "…")]`) becomes one
//!   `InfoNode::Row { label, value }`, the value from the field type's
//!   `InfoValue` impl;
//! - a **nested** field (`#[info(nest)]`) splices in the field's own
//!   `info_nodes()` — a *titled* sub-view (its struct carries `#[info(title)]`)
//!   appears as a nested `InfoNode::Block`; an *untitled* one inlines its rows
//!   into the current block. `Option<T>` nests nothing when `None`.
//!
//! The struct itself is titled by `#[info(title = "…")]` (static) or
//! `#[info(title_from = "method")]` (dynamic — calls `self.method()`), in which
//! case `info_nodes()` yields a single `Block`. A struct with neither is a
//! *container*: it yields its fields' nodes directly (used for the top-level
//! struct of a multi-block type, whose fields are all `#[info(nest)]` blocks).
//!
//! A field is skipped when its skip predicate holds — resolved (highest
//! precedence first) from `#[info(skip_if_zero)]`, `#[info(skip_if = "path")]`,
//! or the field's own `#[serde(skip_serializing_if = "path")]`, so a skip
//! declared once for JSON is mirrored in print unless print must diverge.
//!
//! Generated paths resolve only through `::peek_foundation` (the re-exporter,
//! guaranteed in scope wherever the derive is used), so a call site needs no
//! second dependency.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitStr, parse_macro_input};

/// How the struct supplies its section title.
enum Title {
    /// `#[info(title = "…")]` — a fixed literal.
    Static(String),
    /// `#[info(title_from = "method")]` — `self.method()`, stringified.
    Dynamic(syn::Path),
    /// Neither — a container; fields' nodes are emitted directly.
    Container,
}

/// Per-field skip predicate, in precedence order.
enum Skip {
    /// `#[info(skip_if_zero)]` — hide when numerically zero.
    Zero,
    /// `#[info(skip_if = "path")]` / `#[serde(skip_serializing_if = "path")]`
    /// — hide when `path(&field)` is true.
    Pred(syn::Path),
    /// Always rendered.
    None,
}

/// Whether a field is a scalar row or a nested sub-view.
enum Role {
    /// `#[info(label = "…")]` — a single value row.
    Scalar(String),
    /// `#[info(nest)]` — splice the field's own `info_nodes()`.
    Nest,
}

#[proc_macro_derive(InfoView, attributes(info))]
pub fn derive_info_view(input: TokenStream) -> TokenStream {
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
                    "InfoView requires a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "InfoView can only be derived for structs",
            ));
        }
    };

    let mut stmts = Vec::new();
    for field in fields {
        let ident = field.ident.as_ref().expect("named field");
        let role = field_role(field)?;
        let skip = field_skip(field)?;

        // The node-producing expression for this field, pushed/extended onto
        // `body`.
        let action = match &role {
            Role::Scalar(label) => quote! {
                body.push(::peek_foundation::info::InfoNode::Row {
                    label: #label,
                    value: ::peek_foundation::info::InfoValue::render_value(&self.#ident, theme),
                });
            },
            Role::Nest => quote! {
                body.extend(::peek_foundation::info::InfoView::info_nodes(&self.#ident, theme));
            },
        };

        let stmt = match skip {
            Skip::None => action,
            Skip::Zero => quote! {
                if !::peek_foundation::info::MaybeZero::is_zero_value(&self.#ident) {
                    #action
                }
            },
            Skip::Pred(path) => quote! {
                if !#path(&self.#ident) {
                    #action
                }
            },
        };
        stmts.push(stmt);
    }

    // Wrap the body in a titled Block, or return it directly for a container.
    let result = match title {
        Title::Static(lit) => quote! {
            ::std::vec![::peek_foundation::info::InfoNode::Block {
                title: ::std::string::String::from(#lit),
                body,
            }]
        },
        Title::Dynamic(method) => quote! {
            ::std::vec![::peek_foundation::info::InfoNode::Block {
                title: ::std::string::ToString::to_string(&self.#method()),
                body,
            }]
        },
        Title::Container => quote! { body },
    };

    Ok(quote! {
        impl ::peek_foundation::info::InfoView for #name {
            fn info_nodes(
                &self,
                theme: &::peek_foundation::theme::PeekTheme,
            ) -> ::std::vec::Vec<::peek_foundation::info::InfoNode> {
                let mut body: ::std::vec::Vec<::peek_foundation::info::InfoNode> =
                    ::std::vec::Vec::new();
                #(#stmts)*
                #result
            }
        }
    })
}

/// Read the struct's title mode from its `#[info(...)]` attributes.
fn struct_title(input: &DeriveInput) -> syn::Result<Title> {
    let mut title: Option<Title> = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("info") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("title") {
                let s: LitStr = meta.value()?.parse()?;
                title = Some(Title::Static(s.value()));
                Ok(())
            } else if meta.path.is_ident("title_from") {
                let s: LitStr = meta.value()?.parse()?;
                title = Some(Title::Dynamic(s.parse()?));
                Ok(())
            } else {
                Err(meta.error("unknown `info` struct attribute (expected `title` / `title_from`)"))
            }
        })?;
    }
    Ok(title.unwrap_or(Title::Container))
}

/// Determine whether a field is a scalar row (`label`) or a nested sub-view
/// (`nest`).
fn field_role(field: &syn::Field) -> syn::Result<Role> {
    let mut label = None;
    let mut nest = false;
    for attr in &field.attrs {
        if !attr.path().is_ident("info") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("label") {
                let s: LitStr = meta.value()?.parse()?;
                label = Some(s.value());
            } else if meta.path.is_ident("nest") {
                nest = true;
            } else if meta.input.peek(syn::Token![=]) {
                // skip_if = "…" (read elsewhere) or any other valued key.
                let _: syn::Expr = meta.value()?.parse()?;
            }
            // bare flags read elsewhere (skip_if_zero): nothing to consume.
            Ok(())
        })?;
    }
    match (label, nest) {
        (Some(_), true) => Err(syn::Error::new_spanned(
            field,
            "field cannot be both `#[info(label)]` and `#[info(nest)]`",
        )),
        (Some(l), false) => Ok(Role::Scalar(l)),
        (None, true) => Ok(Role::Nest),
        (None, false) => Err(syn::Error::new_spanned(
            field,
            "field needs `#[info(label = \"…\")]` or `#[info(nest)]`",
        )),
    }
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
                // label = "…" / title-ish valued keys read elsewhere.
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
/// only, not the print row.
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
            Ok(())
        })?;
    }
    Ok(pred)
}
