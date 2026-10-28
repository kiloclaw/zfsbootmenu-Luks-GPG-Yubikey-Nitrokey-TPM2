//! Reading one Rust source file into the facts that the metrics count.
//!
//! [`scan_file`] walks the syntax tree of a file with [`syn::visit`] and
//! records each function (its length, return type and whether it is test
//! code), the out-of-line modules it declares, and a few things the refactor
//! removes: doc comments that name a legacy function, uses of the
//! shell-semantics helpers, `unsafe` blocks, ZFS command lines built by
//! hand, and log macros with a format string.

use std::collections::{BTreeSet, HashSet};

use proc_macro2::{TokenStream, TokenTree};
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, Block, Expr, Lit, Meta, ReturnType, Signature, Token, Type};

/// Helpers that emulate Bash's semantics: those of the former
/// `zbm_core::shell`, and the copies `zbm-kcl` keeps until ROADMAP.md's DM10.
pub const SHELL_HELPERS: &[&str] = &[
    "command_substitution",
    "echo",
    "read_first_line",
    "read_lines",
    "test_integer",
    "trim_ifs",
];

/// The functions that start an interactive shell. SE9 puts them behind one
/// gate, `ShellGate`; until then their calls outside tests are counted, and
/// may only go down.
pub const SHELL_STARTS: &[&str] = &["interactive_shell", "run_recovery_shell"];

/// The ZFS programs, whose command lines `zbm-zfs` alone builds (SB2).
pub const ZFS_PROGRAMS: &[&str] = &["zfs", "zpool", "zdb"];

/// The raw command line builders of `zbm-zfs`, which SB2 replaces with the
/// `Zfs` port everywhere else.
pub const ZFS_BUILDERS: &[&str] = &["ZfsCommand", "ZpoolCommand"];

/// The macros that log, of `zbm_trace` and `tracing`; a message of theirs
/// that is a format string, rather than a constant with fields, is counted
/// (DM9).
pub const LOG_MACROS: &[&str] = &["error", "warn", "notice", "info", "debug", "trace"];

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
    /// Calls of [`SHELL_STARTS`] outside tests.
    pub shell_starts: usize,
    /// `unsafe` blocks.
    pub unsafe_blocks: usize,
    /// ZFS command lines built outside tests: `Command::new` of a
    /// [`ZFS_PROGRAMS`] program, or a [`ZFS_BUILDERS`] builder.
    pub zfs_commands: usize,
    /// Calls of [`LOG_MACROS`] outside tests whose message is a format
    /// string.
    pub format_logs: usize,
    /// The lines of test code in a file that is not test code as a whole:
    /// inline `#[cfg(test)]` modules and test functions, as ranges from
    /// their first line to their last.
    pub test_lines: Vec<(usize, usize)>,
}

/// One function with a body.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent facts about one function, which the metrics count one by one"
)]
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
    /// Whether the function is `pub`.
    pub public: bool,
    /// Whether one of its parameters is a string (`str` or `String`, by
    /// reference or inside another type) rather than a type of its own.
    pub string_params: bool,
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
        recording: 0,
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
    /// How many arguments of `parity.legacy(...)` the walk is inside: legacy
    /// code there runs only when the golden files are recorded.
    recording: usize,
    /// What was found so far.
    scan: FileScan,
}

impl Scanner<'_> {
    /// Records a function, `pub` or not, and walks its body.
    fn function(&mut self, attrs: &[Attribute], public: bool, sig: &Signature, body: &Block) {
        let is_test = attrs.iter().any(is_test_attr);
        let start = sig.fn_token.span.start().line;
        let end = body.brace_token.span.close().end().line;
        let saved = self.test_code;
        self.test_code = saved || is_test || is_cfg_test(attrs);
        if self.test_code {
            self.scan.test_lines.push((start, end));
        }
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
            public: public && !self.trait_impl,
            string_params: sig.inputs.iter().any(|input| match input {
                syn::FnArg::Typed(typed) => mentions_string(&typed.ty),
                syn::FnArg::Receiver(_) => false,
            }),
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

    /// The function the walk is in, if any, unless the walk is inside a
    /// recording of legacy results, which the function does not run.
    fn current_fn(&mut self) -> Option<&mut FnScan> {
        if self.recording > 0 {
            return None;
        }
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
                        function.runs_legacy |= name == "Bash";
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
        if test_code && !saved {
            let start = item.mod_token.span.start().line;
            let end = item
                .content
                .as_ref()
                .map_or(start, |(brace, _)| brace.span.close().end().line);
            self.scan.test_lines.push((start, end));
        }
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
        let public = matches!(item.vis, syn::Visibility::Public(_));
        self.function(&item.attrs, public, &item.sig, &item.block);
        self.trait_impl = saved;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        let public = matches!(item.vis, syn::Visibility::Public(_));
        self.function(&item.attrs, public, &item.sig, &item.block);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        match &item.default {
            Some(body) => self.function(&item.attrs, false, &item.sig, body),
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
            if !self.test_code && SHELL_STARTS.contains(&last.as_str()) {
                self.scan.shell_starts += 1;
            }
            if let Some(function) = self.current_fn() {
                function.runs_legacy |= refers_to_legacy(&segments);
                function.mentions.insert(last.clone());
            }
        }
        visit::visit_expr_path(self, expr);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if !self.test_code && SHELL_STARTS.contains(&call.method.to_string().as_str()) {
            self.scan.shell_starts += 1;
        }
        if call.method != "legacy" {
            visit::visit_expr_method_call(self, call);
            return;
        }
        self.visit_expr(&call.receiver);
        self.recording += 1;
        for arg in &call.args {
            self.visit_expr(arg);
        }
        self.recording -= 1;
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if !self.test_code && is_zfs_command(call) {
            self.scan.zfs_commands += 1;
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_unsafe(&mut self, expr: &'ast syn::ExprUnsafe) {
        self.scan.unsafe_blocks += 1;
        visit::visit_expr_unsafe(self, expr);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        visit::visit_macro(self, mac);
        let logs = mac
            .path
            .segments
            .last()
            .is_some_and(|last| LOG_MACROS.contains(&last.ident.to_string().as_str()));
        if logs && !self.test_code && has_format_message(mac.tokens.clone()) {
            self.scan.format_logs += 1;
        }
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
/// `legacy` module, its `Bash` builder, or its `perl` function. A bare
/// `legacy` is a variable, not the module.
fn refers_to_legacy(segments: &[String]) -> bool {
    (segments.len() > 1 && segments.iter().any(|segment| segment == "legacy"))
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

mod facts;

use facts::{has_format_message, is_zfs_command, mentions_string};

#[cfg(test)]
mod tests;
