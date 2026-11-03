//! What counts as a public item: the items other crates may reach, for
//! the `public_items` metric.

use syn::Attribute;

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
        _ => &[],
    }
}

/// How many public items `item` declares: one for a `pub` const, enum,
/// function, static, struct, trait, type or union; one for each name a
/// `pub use` exports; none for anything else.
pub(super) fn public_names(item: &syn::Item) -> usize {
    let vis = match item {
        syn::Item::Const(item) => &item.vis,
        syn::Item::Enum(item) => &item.vis,
        syn::Item::Fn(item) => &item.vis,
        syn::Item::Static(item) => &item.vis,
        syn::Item::Struct(item) => &item.vis,
        syn::Item::Trait(item) => &item.vis,
        syn::Item::Type(item) => &item.vis,
        syn::Item::Union(item) => &item.vis,
        syn::Item::Use(item) if is_public(&item.vis) => return use_names(&item.tree),
        _ => return 0,
    };
    usize::from(is_public(vis))
}

/// The names a use tree brings in: one per leaf, glob included.
fn use_names(tree: &syn::UseTree) -> usize {
    match tree {
        syn::UseTree::Path(path) => use_names(&path.tree),
        syn::UseTree::Group(group) => group.items.iter().map(use_names).sum(),
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => 1,
    }
}
