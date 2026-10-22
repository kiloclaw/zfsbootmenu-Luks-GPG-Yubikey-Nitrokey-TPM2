//! Reading one Rust source file into the facts that the metrics count.
//!
//! [`scan_file`] walks the syntax tree of a file with [`syn::visit`] and
//! records each function (its length, return type and whether it is test
//! code), the out-of-line modules it declares, and a few things the refactor
//! removes: doc comments that name a legacy function, uses of the
//! shell-semantics helpers, and `unsafe` blocks.

use std::collections::{BTreeSet, HashSet};

use proc_macro2::{TokenStream, TokenTree};
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, Block, Expr, Lit, Meta, ReturnType, Signature, Token, Type};

/// The shell-semantics helpers of `zbm_core::shell`.
pub const SHELL_HELPERS: &[&str] = &[
    "command_substitution",
    "echo",
    "read_first_line",
    "read_lines",
    "test_integer",
    "trim_ifs",
];

/// What a file holds, as far as the metrics are concerned.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileScan {
    /// The number of lines of the file.
    pub lines: usize,
    /// Every function with a body, in source order.
    pub functions: Vec<FnScan>,
    /// The out-of-line modules (`mod name;`) the file declares.
    pub modules: Vec<ModDecl>,
    /// Doc comment lines, outside tests, that name a legacy function.
    pub legacy_docs: usize,
    /// Uses of [`SHELL_HELPERS`] outside tests.
    pub shell_helpers: usize,
    /// `unsafe` blocks.
    pub unsafe_blocks: usize,
}

/// One function with a body.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FnScan {
    /// The function's name.
    pub name: String,
    /// Lines from `fn` to the closing brace.
    pub lines: usize,
    /// What the function returns.
    pub returns: Returns,
    /// Whether the function is a test, test code, or neither.
    pub role: Role,
    /// Whether the function implements a trait method, whose signature the
    /// trait decides.
    pub trait_impl: bool,
    /// Whether the body refers to the legacy test helpers directly.
    pub runs_legacy: bool,
    /// The last segment of every path the body mentions.
    pub mentions: BTreeSet<String>,
}

/// Whether a function belongs to the tests.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Role {
    /// Code that is built without tests.
    #[default]
    Code,
    /// Test code that is not a test itself: in a `#[cfg(test)]` module or
    /// file, for example a helper.
    TestCode,
    /// A test: marked `#[test]`.
    Test,
}

impl Role {
    /// Whether the function is test code, a test or a helper.
    pub fn is_test_code(self) -> bool {
        self != Role::Code
    }

    /// Whether the function is a test.
    pub fn is_test(self) -> bool {
        self == Role::Test
    }
}

/// What a function returns, as far as the metrics care.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Returns {
    /// `bool`.
    Bool,
    /// A type named `Result`.
    Result,
    /// Anything else, including nothing.
    #[default]
    Other,
}

/// An out-of-line module declaration, `mod name;`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModDecl {
    /// The inline modules the declaration is nested in, outermost first.
    pub inline: Vec<String>,
    /// The module's name.
    pub name: String,
    /// Whether the module is test code.
    pub test_code: bool,
}

/// Scans `file`, whose text has `lines` lines.
///
/// `test_code` says whether the whole file is test code, as an integration
/// test or a module declared under `#[cfg(test)]` is. `legacy_names` are the
/// function names of the legacy code.
pub fn scan_file(
    file: &syn::File,
    lines: usize,
    test_code: bool,
    legacy_names: &HashSet<String>,
) -> FileScan {
    let mut scanner = Scanner {
        legacy_names,
        test_code,
        trait_impl: false,
        inline: Vec::new(),
        current: Vec::new(),
        scan: FileScan {
            lines,
            ..FileScan::default()
        },
    };
    scanner.visit_file(file);
    scanner.scan
}

/// The state of the walk over one file.
struct Scanner<'n> {
    /// The function names of the legacy code.
    legacy_names: &'n HashSet<String>,
    /// Whether the walk is inside test code.
    test_code: bool,
    /// Whether the walk is inside `impl Trait for Type`.
    trait_impl: bool,
    /// The inline modules the walk is inside.
    inline: Vec<String>,
    /// Indexes into `scan.functions` of the functions the walk is inside.
    current: Vec<usize>,
    /// What was found so far.
    scan: FileScan,
}

impl Scanner<'_> {
    /// Records a function and walks its body.
    fn function(&mut self, attrs: &[Attribute], sig: &Signature, body: &Block) {
        let is_test = attrs.iter().any(is_test_attr);
        let start = sig.fn_token.span.start().line;
        let end = body.brace_token.span.close().end().line;
        let saved = self.test_code;
        self.test_code = saved || is_test || is_cfg_test(attrs);
        self.scan.functions.push(FnScan {
            name: sig.ident.to_string(),
            lines: end.saturating_sub(start) + 1,
            returns: returns(&sig.output),
            role: if is_test {
                Role::Test
            } else if self.test_code {
                Role::TestCode
            } else {
                Role::Code
            },
            trait_impl: self.trait_impl,
            ..FnScan::default()
        });
        self.current.push(self.scan.functions.len() - 1);
        for attr in attrs {
            self.visit_attribute(attr);
        }
        self.visit_block(body);
        self.current.pop();
        self.test_code = saved;
    }

    /// The function the walk is in, if any.
    fn current_fn(&mut self) -> Option<&mut FnScan> {
        let index = *self.current.last()?;
        self.scan.functions.get_mut(index)
    }

    /// Records the identifiers of macro input that does not parse as
    /// expressions, for example `matches!` with a pattern.
    fn macro_tokens(&mut self, tokens: TokenStream) {
        for tree in tokens {
            match tree {
                TokenTree::Group(group) => self.macro_tokens(group.stream()),
                TokenTree::Ident(ident) => {
                    let name = ident.to_string();
                    if let Some(function) = self.current_fn() {
                        function.runs_legacy |= name == "legacy" || name == "Bash";
                        function.mentions.insert(name);
                    }
                }
                TokenTree::Punct(_) | TokenTree::Literal(_) => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let test_code = self.test_code || is_cfg_test(&item.attrs);
        let Some((_, items)) = &item.content else {
            self.scan.modules.push(ModDecl {
                inline: self.inline.clone(),
                name: item.ident.to_string(),
                test_code,
            });
            return;
        };
        let saved = self.test_code;
        self.test_code = test_code;
        self.inline.push(item.ident.to_string());
        for attr in &item.attrs {
            self.visit_attribute(attr);
        }
        for item in items {
            self.visit_item(item);
        }
        self.inline.pop();
        self.test_code = saved;
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let saved = (self.trait_impl, self.test_code);
        self.trait_impl = item.trait_.is_some();
        self.test_code = saved.1 || is_cfg_test(&item.attrs);
        visit::visit_item_impl(self, item);
        (self.trait_impl, self.test_code) = saved;
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        let saved = self.trait_impl;
        self.trait_impl = false;
        visit::visit_item_trait(self, item);
        self.trait_impl = saved;
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let saved = self.trait_impl;
        self.trait_impl = false;
        self.function(&item.attrs, &item.sig, &item.block);
        self.trait_impl = saved;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.function(&item.attrs, &item.sig, &item.block);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        match &item.default {
            Some(body) => self.function(&item.attrs, &item.sig, body),
            None => visit::visit_trait_item_fn(self, item),
        }
    }

    fn visit_attribute(&mut self, attr: &'ast Attribute) {
        if self.test_code {
            return;
        }
        if let Some(text) = doc_text(attr)
            && names_legacy_function(&text, self.legacy_names)
        {
            self.scan.legacy_docs += 1;
        }
    }

    fn visit_expr_path(&mut self, expr: &'ast syn::ExprPath) {
        let segments: Vec<String> = expr
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if let Some(last) = segments.last() {
            if !self.test_code && SHELL_HELPERS.contains(&last.as_str()) {
                self.scan.shell_helpers += 1;
            }
            if let Some(function) = self.current_fn() {
                function.runs_legacy |= refers_to_legacy(&segments);
                function.mentions.insert(last.clone());
            }
        }
        visit::visit_expr_path(self, expr);
    }

    fn visit_expr_unsafe(&mut self, expr: &'ast syn::ExprUnsafe) {
        self.scan.unsafe_blocks += 1;
        visit::visit_expr_unsafe(self, expr);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        visit::visit_macro(self, mac);
        let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
        match mac.parse_body_with(parser) {
            Ok(exprs) => {
                for expr in &exprs {
                    visit_owned_expr(self, expr);
                }
            }
            Err(_) => self.macro_tokens(mac.tokens.clone()),
        }
    }
}

/// Walks an expression that the syntax tree does not own: one parsed from
/// the input of a macro.
fn visit_owned_expr(scanner: &mut Scanner<'_>, expr: &Expr) {
    scanner.visit_expr(expr);
}

/// Whether a path names the legacy test helpers of `zbm_testkit`: the
/// `legacy` module, its `Bash` builder, or its `perl` function.
fn refers_to_legacy(segments: &[String]) -> bool {
    segments.iter().any(|segment| segment == "legacy")
        || segments.first().is_some_and(|first| first == "Bash")
        || segments == ["perl"]
}

/// What `output` returns.
fn returns(output: &ReturnType) -> Returns {
    let ReturnType::Type(_, ty) = output else {
        return Returns::Other;
    };
    let Type::Path(path) = ty.as_ref() else {
        return Returns::Other;
    };
    match path.path.segments.last() {
        Some(last) if last.ident == "bool" => Returns::Bool,
        Some(last) if last.ident == "Result" => Returns::Result,
        _ => Returns::Other,
    }
}

/// Whether `attr` is `#[test]` or a test attribute of another crate, such as
/// `#[tokio::test]`.
fn is_test_attr(attr: &Attribute) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "test")
}

/// Whether `attrs` hold a `#[cfg(...)]` that compiles the item only for
/// tests.
pub fn is_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| match &attr.meta {
        Meta::List(list) if list.path.is_ident("cfg") => requires_test(list.tokens.clone()),
        _ => false,
    })
}

/// Whether the `cfg` predicate `tokens` is true only when testing.
///
/// `test` is; `all(...)` is when one of its parts is; `any(...)` is when all
/// of its parts are; `not(...)` and everything else is not.
fn requires_test(tokens: TokenStream) -> bool {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    match trees.as_slice() {
        [TokenTree::Ident(ident)] => ident == "test",
        [TokenTree::Ident(ident), TokenTree::Group(group)] => {
            let parts = split_commas(group.stream());
            if ident == "all" {
                parts.into_iter().any(requires_test)
            } else if ident == "any" {
                !parts.is_empty() && parts.into_iter().all(requires_test)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// The comma-separated parts of `tokens`.
fn split_commas(tokens: TokenStream) -> Vec<TokenStream> {
    let mut parts = vec![TokenStream::new()];
    for tree in tokens {
        match &tree {
            TokenTree::Punct(punct) if punct.as_char() == ',' => parts.push(TokenStream::new()),
            _ => {
                if let Some(part) = parts.last_mut() {
                    part.extend([tree]);
                }
            }
        }
    }
    parts.retain(|part| !part.is_empty());
    parts
}

/// The text of a doc attribute (`///` or `//!`), if `attr` is one.
fn doc_text(attr: &Attribute) -> Option<String> {
    let Meta::NameValue(pair) = &attr.meta else {
        return None;
    };
    if !pair.path.is_ident("doc") {
        return None;
    }
    let Expr::Lit(literal) = &pair.value else {
        return None;
    };
    let Lit::Str(text) = &literal.lit else {
        return None;
    };
    Some(text.value())
}

/// Whether `text` names one of `names` as code: the first word between
/// backticks, as in "as `luks_open` does".
pub fn names_legacy_function(text: &str, names: &HashSet<String>) -> bool {
    text.split('`').skip(1).step_by(2).any(|code| {
        let word = code
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .unwrap_or_default();
        names.contains(word)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(source: &str) -> FileScan {
        scan_test(source, false)
    }

    fn scan_test(source: &str, test_code: bool) -> FileScan {
        let names: HashSet<String> = ["luks_open".to_owned()].into();
        let file = syn::parse_file(source).unwrap();
        scan_file(&file, source.lines().count(), test_code, &names)
    }

    #[test]
    fn functions_are_measured_from_fn_to_the_closing_brace() {
        let file = scan("fn a() -> bool {\n    true\n}\n\nfn b() {}\n");

        let lengths: Vec<usize> = file.functions.iter().map(|f| f.lines).collect();
        assert_eq!(lengths, [3, 1]);
        assert_eq!(file.functions[0].returns, Returns::Bool);
        assert_eq!(file.lines, 5);
    }

    #[test]
    fn result_types_are_recognised_by_their_last_segment() {
        let file = scan("fn a() -> io::Result<()> { Ok(()) }\nfn b() -> Option<u8> { None }");

        assert_eq!(file.functions[0].returns, Returns::Result);
        assert_eq!(file.functions[1].returns, Returns::Other);
    }

    #[test]
    fn functions_in_cfg_test_modules_are_test_code() {
        let source = "fn run() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn runs() {}\n}\n";

        let file = scan(source);

        assert_eq!(file.functions[0].role, Role::Code);
        assert_eq!(file.functions[1].role, Role::Test);
    }

    #[test]
    fn cfg_not_test_is_not_test_code() {
        let file = scan("#[cfg(not(test))]\nmod real {\n    fn run() {}\n}\n");

        assert_eq!(file.functions[0].role, Role::Code);
    }

    #[test]
    fn cfg_all_with_test_is_test_code() {
        let file = scan("#[cfg(all(test, unix))]\nmod tests {\n    fn helper() {}\n}\n");

        assert_eq!(file.functions[0].role, Role::TestCode);
    }

    #[test]
    fn trait_methods_are_marked() {
        let source = "impl PartialEq for X {\n    fn eq(&self, _: &X) -> bool { true }\n}\n";

        let file = scan(source);

        assert!(file.functions[0].trait_impl);
    }

    #[test]
    fn out_of_line_modules_are_recorded_with_their_inline_parents() {
        let source = "mod a;\nmod outer {\n    #[cfg(test)]\n    mod b;\n}\n";

        let file = scan(source);

        assert_eq!(
            file.modules,
            [
                ModDecl {
                    inline: Vec::new(),
                    name: "a".to_owned(),
                    test_code: false,
                },
                ModDecl {
                    inline: vec!["outer".to_owned()],
                    name: "b".to_owned(),
                    test_code: true,
                },
            ]
        );
    }

    #[test]
    fn doc_comments_that_name_legacy_functions_are_counted_outside_tests() {
        let source = "/// Opens a volume as `luks_open` does.\nfn open() {}\n\
                      /// Not code: luks_open.\nfn other() {}\n";

        assert_eq!(scan(source).legacy_docs, 1);
        assert_eq!(scan_test(source, true).legacy_docs, 0);
    }

    #[test]
    fn shell_helpers_are_counted_outside_tests_including_in_macros() {
        let source = "fn a(x: &str) {\n    let n = test_integer(x);\n    \
                      println!(\"{}\", shell::echo(x));\n}\n";

        assert_eq!(scan(source).shell_helpers, 2);
        assert_eq!(scan_test(source, true).shell_helpers, 0);
    }

    #[test]
    fn unsafe_blocks_are_counted() {
        let file = scan("fn a() { unsafe { f() }; unsafe { g() } }");

        assert_eq!(file.unsafe_blocks, 2);
    }

    #[test]
    fn calls_to_the_legacy_helpers_are_noticed_also_in_assertions() {
        let source = "#[test]\nfn parity() {\n    \
                      assert_eq!(Bash::new().call(\"f\", &[], b\"\"), rust());\n}\n\
                      #[test]\nfn plain() {\n    assert_eq!(rust(), 1);\n}\n";

        let file = scan_test(source, true);

        assert!(file.functions[0].runs_legacy);
        assert!(!file.functions[1].runs_legacy);
        assert!(file.functions[1].mentions.contains("rust"));
    }

    #[test]
    fn perl_counts_only_as_the_bare_helper() {
        let source = "fn a() { perl(\"x\", &[], b\"\"); }\nfn b() { crate::perl::quote(); }\n";

        let file = scan_test(source, true);

        assert!(file.functions[0].runs_legacy);
        assert!(!file.functions[1].runs_legacy);
    }

    #[test]
    fn a_legacy_name_must_be_the_first_word_in_backticks() {
        let names: HashSet<String> = ["luks_open".to_owned()].into();

        assert!(names_legacy_function(
            "as `luks_open \"$dev\"` does",
            &names
        ));
        assert!(!names_legacy_function(
            "as `cryptsetup luks_open` does",
            &names
        ));
        assert!(!names_legacy_function("luks_open", &names));
    }
}
