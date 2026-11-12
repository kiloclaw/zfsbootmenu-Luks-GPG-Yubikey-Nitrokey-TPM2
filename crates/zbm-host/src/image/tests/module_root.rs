//! What Rust plans take of the module root's `lib`, `libexec` and `bin`:
//! the library and `zfsbootmenu-input` (ADR 0018, point 22).

use super::*;

/// The lines of a Rust plan that take a file of the module root, sorted:
/// the legacy copies of the library and the line reader.
fn rust_takes() -> Vec<String> {
    let mut taken: Vec<String> = library::FILES
        .iter()
        .map(|file| {
            let legacy = format!("{}/lib/{file}", library::LEGACY);
            format!("file\t{MODULE_ROOT}/lib/{file}\t{legacy}\t-")
        })
        .collect();
    let input = "libexec/zfsbootmenu-input";
    taken.push(format!("file\t{MODULE_ROOT}/{input}\t/{input}\t-"));
    taken.sort_unstable();
    taken
}

/// The lines of `golden` that take a file of the module root's `dir`, or
/// put one of its names in the image other than as a link to `zbm`.
fn taken_from<'a>(golden: &'a str, dir: &str) -> Vec<&'a str> {
    let names: Vec<String> = std::fs::read_dir(checkout_root().join(dir))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    golden
        .lines()
        .filter(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            names.iter().any(|name| {
                let src = format!("{MODULE_ROOT}/{dir}/{name}");
                let dest = format!("/{dir}/{name}");
                let is_file = fields.get(1) == Some(&src.as_str());
                let is_link = fields.first() == Some(&"link")
                    && fields.get(1) != Some(&"/libexec/zbm")
                    && fields.get(2) == Some(&dest.as_str());
                is_file || is_link
            })
        })
        .collect()
}

#[test]
fn a_rust_plan_takes_only_the_library_and_zfsbootmenu_input_of_the_module_root() {
    let (files, runner) = (host_files(), host_runner());
    for (builder, settings) in RUST {
        let golden = plan_of(builder, settings, &files, &runner)
            .unwrap()
            .golden();
        let mut taken: Vec<&str> = ["lib", "libexec", "bin"]
            .into_iter()
            .flat_map(|dir| taken_from(&golden, dir))
            .collect();
        taken.sort_unstable();
        assert_eq!(taken, rust_takes(), "{builder:?}");
        // The library itself is generated.
        for file in library::FILES {
            let line = format!("\nfile\t@/lib/{file}\t/lib/{file}\t0644\n");
            assert!(golden.contains(&line), "{builder:?}: {file}");
        }
    }
    // Legacy plans take every file, the LUKS libraries among them.
    let legacy = plan_of(Builder::Dracut, "", &files, &runner)
        .unwrap()
        .golden();
    let luks = format!("file\t{MODULE_ROOT}/lib/zfsbootmenu-luks.sh\t/lib/zfsbootmenu-luks.sh\t-");
    assert!(taken_from(&legacy, "lib").contains(&luks.as_str()));
}
