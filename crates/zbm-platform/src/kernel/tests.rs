use super::*;

#[test]
fn the_real_loader_reads_the_files_and_the_kernel_refuses_what_is_no_kernel() {
    let missing = SystemKernelLoader
        .load(
            Path::new("/nonexistent/vmlinuz"),
            Path::new("/dev/null"),
            "quiet",
        )
        .unwrap_err();
    assert_eq!(missing.kind(), io::ErrorKind::NotFound);
    let nul = SystemKernelLoader
        .load(Path::new("/dev/null"), Path::new("/dev/null"), "a\0b")
        .unwrap_err();
    assert_eq!(nul.kind(), io::ErrorKind::InvalidInput);
    // Without `CAP_SYS_BOOT`, or as no kernel: refused, and a kernel loaded
    // before stays loaded.
    assert!(
        SystemKernelLoader
            .load(Path::new("/dev/null"), Path::new("/dev/null"), "quiet")
            .is_err()
    );
}

#[test]
fn no_loader_leaves_both_to_the_program() {
    let none = NoKernelLoader.load(Path::new("/k"), Path::new("/i"), "");
    assert_eq!(none.unwrap_err().kind(), io::ErrorKind::Unsupported);
    assert_eq!(NoKernelLoader.start().kind(), io::ErrorKind::Unsupported);
}
