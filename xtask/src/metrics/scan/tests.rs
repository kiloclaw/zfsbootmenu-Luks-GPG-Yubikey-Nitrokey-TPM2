//! Tests of the source scanner.

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
fn interactive_shell_starts_are_counted_outside_tests() {
    let source = "fn a(r: &R) {\n    let _ = programs::interactive_shell(r);\n    \
                  self.run_recovery_shell(&self.shell);\n}\nfn interactive_shell() {}\n";

    assert_eq!(scan(source).shell_starts, 2);
    assert_eq!(scan_test(source, true).shell_starts, 0);
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
fn legacy_code_inside_a_recording_does_not_count() {
    let source = "#[test]\nfn recorded() {\n    \
                  let r = parity.legacy(\"x\", || legacy_core().call(\"f\", &[], b\"\"));\n    \
                  assert_eq!(rust(), r);\n}\n\
                  fn legacy_core() -> Bash { Bash::new() }\n";

    let file = scan(source);

    assert!(!file.functions[0].runs_legacy);
    assert!(!file.functions[0].mentions.contains("legacy_core"));
    assert!(file.functions[1].runs_legacy);
}

#[test]
fn a_variable_named_legacy_is_not_the_legacy_module() {
    let file = scan("#[test]\nfn t() {\n    let legacy = 1;\n    assert_eq!(legacy, 1);\n}\n");

    assert!(!file.functions[0].runs_legacy);
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

#[test]
fn zfs_command_lines_are_counted_outside_tests() {
    let source = r#"
fn list() {
    let a = Command::new("zfs").arg("list");
    let b = zbm_platform::Command::new("zpool");
    let c = ZfsCommand::new(runner).get("x");
    let d = ZpoolCommand::new(runner);
    let e = Command::new("cryptsetup");
    let f = Command::new(program);
}
"#;

    assert_eq!(scan(source).zfs_commands, 4);
    assert_eq!(scan_test(source, true).zfs_commands, 0);
}

#[test]
fn public_functions_that_take_strings_are_marked() {
    let source = r"
pub fn by_name(name: &str) {}
pub fn by_names(names: &[String], pool: Option<&str>) {}
pub fn typed(pool: &PoolName, count: usize) {}
fn private(name: &str) {}
pub(crate) fn inner(name: String) {}
struct S;
impl S {
    pub fn method(&self, device: &str) {}
}
impl Display for S {
    fn fmt(&self, text: &str) {}
}
";

    let file = scan(source);

    let marked: Vec<&str> = file
        .functions
        .iter()
        .filter(|function| function.public && function.string_params)
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(marked, ["by_name", "by_names", "method"]);
}

#[test]
fn log_macros_with_a_format_string_are_counted() {
    let source = r#"
fn log(pool: &str) {
    zbm_trace::info!("importing {pool}");
    tracing::warn!(target: "zbm", "failed: {}", pool);
    tracing::info!(%pool, "imported");
    debug!(count = 3, "done");
    error!("a brace {{ shown as is }}");
    println!("not a log {pool}");
    zbm_trace::debug!("pool set to {pool}"; %pool);
    zbm_trace::warn!("{pool} is {state}"; pool, state = ?pool);
    zbm_trace::warn!("{pool} is {missing}"; %pool);
}
"#;

    // The last template names a field it does not have.
    assert_eq!(scan(source).format_logs, 3);
    assert_eq!(scan_test(source, true).format_logs, 0);
}

#[test]
fn the_lines_of_inline_test_code_are_recorded() {
    let source = "fn run() {}\n\
                  #[cfg(test)]\n\
                  mod tests {\n\
                      #[test]\n\
                      fn runs() {}\n\
                  }\n\
                  #[test]\n\
                  fn alone() {\n\
                  }\n";

    let file = scan(source);

    assert_eq!(file.test_lines, [(3, 6), (5, 5), (8, 9)]);
}
