//! What a source file of the menu says: its text literals, and the message
//! ids it names with `text!` and `id!`.

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::{self, Visit};
use syn::{Attribute, Item, LitStr};

use crate::metrics::scan::is_cfg_test;

/// The macros whose first argument is a message id.
pub const ID_MACROS: &[&str] = &["text", "id"];

/// A string literal of the source: its line and its value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Found {
    /// The line it starts on.
    pub line: usize,
    /// Its value.
    pub value: String,
}

/// What one file says.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TextScan {
    /// The text literals outside test code.
    pub literals: Vec<Found>,
    /// The message ids named outside test code.
    pub ids: Vec<Found>,
    /// The message ids named in test code.
    pub test_ids: Vec<Found>,
}

/// Scans `file`, all of which is test code when `test_code` is set.
pub fn scan(file: &syn::File, test_code: bool) -> TextScan {
    let mut visitor = Visitor {
        scan: TextScan::default(),
        test_code,
    };
    visitor.visit_file(file);
    visitor.scan
}

/// Whether `value` reads as text: it has a letter outside the placeholders
/// of a format string (`{name}`, `{0:>8}`).
pub fn is_text(value: &str) -> bool {
    let mut depth = 0_usize;
    for c in value.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            c if depth == 0 && c.is_alphabetic() => return true,
            _ => {}
        }
    }
    false
}

/// The attributes of `item`, for the kinds that can be test code.
fn attributes(item: &Item) -> &[Attribute] {
    match item {
        Item::Fn(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Const(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

/// The walk over one file.
struct Visitor {
    scan: TextScan,
    test_code: bool,
}

impl Visitor {
    /// Records a literal, as text or as a message id.
    fn found(&mut self, line: usize, value: String, is_id: bool) {
        let found = Found { line, value };
        if is_id && self.test_code {
            self.scan.test_ids.push(found);
        } else if is_id {
            self.scan.ids.push(found);
        } else if !self.test_code && is_text(&found.value) {
            self.scan.literals.push(found);
        }
    }

    /// Walks the tokens of a macro call: its first literal is a message id
    /// when `id_first` is set, and calls of [`ID_MACROS`] inside it name ids
    /// too.
    fn tokens(&mut self, stream: TokenStream, id_first: bool) {
        let mut trees = stream.into_iter().peekable();
        let mut first = id_first;
        while let Some(tree) = trees.next() {
            match tree {
                TokenTree::Ident(ident) if ID_MACROS.iter().any(|name| ident == name) => {
                    let bang =
                        matches!(trees.peek(), Some(TokenTree::Punct(p)) if p.as_char() == '!');
                    if bang {
                        trees.next();
                        if let Some(TokenTree::Group(group)) = trees.next() {
                            self.tokens(group.stream(), true);
                        }
                    }
                }
                TokenTree::Group(group) => self.tokens(group.stream(), false),
                TokenTree::Literal(literal) => {
                    if let Ok(string) = syn::parse_str::<LitStr>(&literal.to_string()) {
                        let line = literal.span().start().line;
                        self.found(line, string.value(), first);
                    }
                }
                TokenTree::Ident(_) | TokenTree::Punct(_) => {}
            }
            first = false;
        }
    }
}

impl<'ast> Visit<'ast> for Visitor {
    fn visit_item(&mut self, item: &'ast Item) {
        let saved = self.test_code;
        self.test_code = saved || is_cfg_test(attributes(item));
        visit::visit_item(self, item);
        self.test_code = saved;
    }

    fn visit_attribute(&mut self, _attr: &'ast Attribute) {
        // Doc comments and attributes are not text the menu shows.
    }

    fn visit_lit_str(&mut self, literal: &'ast LitStr) {
        self.found(literal.span().start().line, literal.value(), false);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let name = mac
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default();
        self.tokens(mac.tokens.clone(), ID_MACROS.contains(&name.as_str()));
    }
}
