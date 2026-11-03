//! What counts as a public item: the items other crates may reach, for
//! the `public_items` metric.

use syn::Attribute;

/// The macros of the workspace whose call declares one public item where
/// it stands: `zbm_types::error_codes!` declares the `pub const` table it
/// names. The items other macros generate are not seen.
const ITEM_MACROS: &[&str] = &["error_codes"];

/// Whether `vis` is plain `pub`, which other crates may reach.
pub(super) fn is_public(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

/// The attributes of `item`, for the kinds that count as public items.
pub(super) fn item_attrs(item: &syn::Item) -> &[Attribute] {
    match item {
        syn::Item::Const(item) => &item.attrs,
        syn::Item::Enum(item) => &item.attrs,
        syn::Item::Fn(item) => &item.attrs,
        syn::Item::Static(item) => &item.attrs,
        syn::Item::Struct(item) => &item.attrs,
        syn::Item::Trait(item) => &item.attrs,
        syn::Item::Type(item) => &item.attrs,
        syn::Item::Union(item) => &item.attrs,
        syn::Item::Use(item) => &item.attrs,
        syn::Item::Macro(item) => &item.attrs,
        _ => &[],
    }
}

/// How many public items `item` declares: one for a `pub` const, enum,
/// function, static, struct, trait, type or union; one for each name a
/// `pub use` exports, a glob counting as one; one for a call of one of the
/// [`ITEM_MACROS`]; none for anything else.
pub(super) fn public_names(item: &syn::Item) -> usize {
    let vis =
        match item {
            syn::Item::Const(item) => &item.vis,
            syn::Item::Enum(item) => &item.vis,
            syn::Item::Fn(item) => &item.vis,
            syn::Item::Static(item) => &item.vis,
            syn::Item::Struct(item) => &item.vis,
            syn::Item::Trait(item) => &item.vis,
            syn::Item::Type(item) => &item.vis,
            syn::Item::Union(item) => &item.vis,
            syn::Item::Use(item) if is_public(&item.vis) => return use_names(&item.tree),
            syn::Item::Macro(item) if item.ident.is_none() => {
                return usize::from(
                    item.mac.path.segments.last().is_some_and(|segment| {
                        ITEM_MACROS.iter().any(|name| segment.ident == name)
                    }),
                );
            }
            _ => return 0,
        };
    usize::from(is_public(vis))
}

/// Whether `item` is a `#[macro_export]` macro, which other crates reach
/// at the crate's root wherever it is defined.
pub(super) fn is_exported_macro(item: &syn::Item) -> bool {
    matches!(item, syn::Item::Macro(item)
        if item.ident.is_some()
            && item.attrs.iter().any(|attr| attr.path().is_ident("macro_export")))
}

/// The names a use tree brings in: one per leaf, glob included.
fn use_names(tree: &syn::UseTree) -> usize {
    match tree {
        syn::UseTree::Path(path) => use_names(&path.tree),
        syn::UseTree::Group(group) => group.items.iter().map(use_names).sum(),
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => 1,
    }
}
