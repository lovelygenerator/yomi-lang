//! Generating the code that `#[derive(YomiType)]` emits (as strings; independent of `proc_macro`).

use crate::parse::{Item, ItemKind};

/// The path that refers to the `yomi-core` crate in the generated code.
const Y: &str = "::yomi_core";

/// Makes the source of the implementations of `YomiType`, `YomiDecl`, `IntoValue` and `FromValue` from the parsed type.
pub fn generate(item: &Item) -> String {
    let (name, full) = (&item.rust_name, &item.full_name);
    let full_lit = format!("{full:?}");
    let ty_ctor = match item.kind {
        ItemKind::Struct(_) => "Struct",
        ItemKind::Enum(_) => "Enum",
    };
    let mut out = String::new();
    out.push_str(&format!(
        "impl {Y}::YomiType for {name} {{ fn ty() -> {Y}::Ty {{ {Y}::Ty::{ty_ctor}(::std::string::ToString::to_string({full_lit})) }} }}\n"
    ));
    let (decl_kind, into_body, from_body) = match &item.kind {
        ItemKind::Struct(fields) => {
            let decl_fields: Vec<String> = fields
                .iter()
                .map(|(f, t)| {
                    format!(
                        "(::std::string::ToString::to_string({f:?}), <{t} as {Y}::YomiType>::ty())"
                    )
                })
                .collect();
            let into_fields: Vec<String> = fields
                .iter()
                .map(|(f, _)| {
                    format!(
                        "(::std::string::ToString::to_string({f:?}), {Y}::IntoValue::into_value(self.{f}))"
                    )
                })
                .collect();
            let from_fields: Vec<String> = fields
                .iter()
                .map(|(f, _)| {
                    format!(
                        "{f}: {Y}::FromValue::from_value(&fields.iter().find(|(n, _)| n == {f:?})?.1)?"
                    )
                })
                .collect();
            (
                format!(
                    "{Y}::TypeKind::Struct {{ fields: ::std::vec![{}] }}",
                    decl_fields.join(", ")
                ),
                format!(
                    "{Y}::Value::Struct(::std::sync::Arc::new({Y}::StructValue {{ ty: ::std::convert::From::from({full_lit}), fields: ::std::vec![{}] }}))",
                    into_fields.join(", ")
                ),
                format!(
                    "match v {{ {Y}::Value::Struct(st) if &*st.ty == {full_lit} => {{ let fields = &st.fields; ::core::option::Option::Some(Self {{ {} }}) }}, _ => ::core::option::Option::None }}",
                    from_fields.join(", ")
                ),
            )
        }
        ItemKind::Enum(variants) => {
            let decl_variants: Vec<String> = variants
                .iter()
                .map(|(v, args)| {
                    let tys: Vec<String> = args
                        .iter()
                        .map(|t| format!("<{t} as {Y}::YomiType>::ty()"))
                        .collect();
                    format!(
                        "{Y}::VariantDecl {{ name: ::std::string::ToString::to_string({v:?}), args: ::std::vec![{}] }}",
                        tys.join(", ")
                    )
                })
                .collect();
            let into_arms: Vec<String> = variants
                .iter()
                .map(|(v, args)| {
                    let binds: Vec<String> = (0..args.len()).map(|i| format!("a{i}")).collect();
                    let pat = if args.is_empty() {
                        format!("Self::{v}")
                    } else {
                        format!("Self::{v}({})", binds.join(", "))
                    };
                    let vals: Vec<String> = binds
                        .iter()
                        .map(|b| format!("{Y}::IntoValue::into_value({b})"))
                        .collect();
                    format!(
                        "{pat} => {Y}::Value::Enum(::std::sync::Arc::new({Y}::EnumValue {{ ty: ::std::convert::From::from({full_lit}), variant: ::std::convert::From::from({v:?}), args: ::std::vec![{}] }}))",
                        vals.join(", ")
                    )
                })
                .collect();
            let from_arms: Vec<String> = variants
                .iter()
                .map(|(v, args)| {
                    if args.is_empty() {
                        format!(
                            "{v:?} if args.is_empty() => ::core::option::Option::Some(Self::{v})"
                        )
                    } else {
                        let vals: Vec<String> = (0..args.len())
                            .map(|i| format!("{Y}::FromValue::from_value(&args[{i}])?"))
                            .collect();
                        format!(
                            "{v:?} if args.len() == {} => ::core::option::Option::Some(Self::{v}({}))",
                            args.len(),
                            vals.join(", ")
                        )
                    }
                })
                .collect();
            (
                format!(
                    "{Y}::TypeKind::Enum {{ variants: ::std::vec![{}] }}",
                    decl_variants.join(", ")
                ),
                format!("match self {{ {} }}", into_arms.join(", ")),
                format!(
                    "match v {{ {Y}::Value::Enum(e) if &*e.ty == {full_lit} => {{ let args = &e.args; match &*e.variant {{ {}, _ => ::core::option::Option::None }} }}, _ => ::core::option::Option::None }}",
                    from_arms.join(", ")
                ),
            )
        }
    };
    out.push_str(&format!(
        "impl {Y}::YomiDecl for {name} {{ fn decl() -> {Y}::TypeDecl {{ {Y}::TypeDecl {{ name: ::std::string::ToString::to_string({full_lit}), kind: {decl_kind} }} }} }}\n"
    ));
    out.push_str(&format!(
        "impl {Y}::IntoValue for {name} {{ fn into_value(self) -> {Y}::Value {{ {into_body} }} }}\n"
    ));
    out.push_str(&format!(
        "impl {Y}::FromValue for {name} {{ fn from_value(v: &{Y}::Value) -> ::core::option::Option<Self> {{ {from_body} }} }}\n"
    ));
    out
}
