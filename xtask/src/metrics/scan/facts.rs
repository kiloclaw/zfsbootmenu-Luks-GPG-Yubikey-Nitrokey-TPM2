//! What the scanner tells of a call, a macro's input or a type: ZFS
//! command lines, format-string messages and string parameters.

use proc_macro2::{TokenStream, TokenTree};
use syn::{Expr, Lit, Type};

use super::{ZFS_BUILDERS, ZFS_PROGRAMS};

/// Whether `call` builds a ZFS command line: `Command::new("zfs")` or
/// another of [`ZFS_PROGRAMS`], or `ZfsCommand::new` and the other
/// [`ZFS_BUILDERS`].
pub(super) fn is_zfs_command(call: &syn::ExprCall) -> bool {
    let Expr::Path(function) = call.func.as_ref() else {
        return false;
    };
    let names: Vec<String> = function
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    let builder = names.len() >= 2
        && names.last().is_some_and(|last| last == "new")
        && names
            .get(names.len() - 2)
            .is_some_and(|name| ZFS_BUILDERS.contains(&name.as_str()));
    if builder {
        return true;
    }
    if !names.ends_with(&["Command".to_owned(), "new".to_owned()]) {
        return false;
    }
    matches!(
        call.args.first(),
        Some(Expr::Lit(syn::ExprLit { lit: Lit::Str(program), .. }))
            if ZFS_PROGRAMS.contains(&program.value().as_str())
    )
}

/// Whether the message of a log macro's input formats something: it holds
/// a `{` that is not `{{`. The message is the first string literal at the
/// top level that starts the input or follows a comma; one after `:` or `=`
/// is the target or a field's value (`target: "x", key = "y",`).
pub(super) fn has_format_message(tokens: TokenStream) -> bool {
    let mut starts_part = true;
    for tree in tokens {
        match &tree {
            TokenTree::Literal(literal) if starts_part => {
                if let Ok(Lit::Str(text)) = syn::parse_str::<Lit>(&literal.to_string()) {
                    return text.value().replace("{{", "").contains('{');
                }
                starts_part = false;
            }
            TokenTree::Punct(punct) => starts_part = punct.as_char() == ',',
            _ => starts_part = false,
        }
    }
    false
}

/// Whether the type `ty` is a string, or holds one: `&str`, `String`,
/// `Option<&str>`, `&[String]` and the like.
pub(super) fn mentions_string(ty: &Type) -> bool {
    match ty {
        Type::Reference(reference) => mentions_string(&reference.elem),
        Type::Slice(slice) => mentions_string(&slice.elem),
        Type::Array(array) => mentions_string(&array.elem),
        Type::Path(path) => path.path.segments.iter().any(|segment| {
            segment.ident == "str"
                || segment.ident == "String"
                || match &segment.arguments {
                    syn::PathArguments::AngleBracketed(generics) => {
                        generics.args.iter().any(|arg| match arg {
                            syn::GenericArgument::Type(inner) => mentions_string(inner),
                            _ => false,
                        })
                    }
                    _ => false,
                }
        }),
        _ => false,
    }
}
