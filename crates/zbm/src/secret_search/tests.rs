use std::fmt::Write as _;

use pretty_assertions::assert_eq;

use super::*;

const SECRET: &[u8] = b"zbm-secret-search-test";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

#[test]
fn secrets_are_given_in_hexadecimal() {
    assert_eq!(decode_hex("70770a"), Some(b"pw\n".to_vec()));
    assert_eq!(decode_hex(&hex(SECRET)).as_deref(), Some(SECRET));
    for bad in ["", "7", "zz", "7g"] {
        assert_eq!(decode_hex(bad), None, "{bad}");
    }
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };
    assert_eq!(run(&["xyz"], &mut io), USAGE);
    assert_eq!(run(&[], &mut io), USAGE);
}

#[test]
fn the_readable_mappings_are_listed() {
    let maps = "\
5600-5700 r--p 00000000 08:01 12 /usr/bin/zbm
5700-5800 ---p 00000000 00:00 0
7ff0-7ff8 rw-p 00000000 00:00 0                          [stack]
7ffa-7ffb r--p 00000000 00:00 0                          [vvar]
bad line
";
    assert_eq!(
        readable_regions(maps),
        [
            (0x5600, 0x5700, "/usr/bin/zbm".to_owned()),
            (0x7ff0, 0x7ff8, "[stack]".to_owned()),
        ]
    );
}

#[test]
fn the_file_systems_in_memory_are_found() {
    let mounts = "rootfs / rootfs rw 0 0\nproc /proc proc rw 0 0\n\
                  tmpfs /run tmpfs rw 0 0\n/dev/vda1 /boot vfat rw 0 0\n\
                  none /a\\040b ramfs rw 0 0\ntmpfs /run tmpfs rw 0 0\n\
                  devtmpfs /dev devtmpfs rw 0 0\n";
    assert_eq!(
        memory_mounts(mounts),
        [
            PathBuf::from("/"),
            PathBuf::from("/a b"),
            PathBuf::from("/dev"),
            PathBuf::from("/run")
        ]
    );
}

/// Memory whose page `hole` cannot be read.
struct Holed {
    bytes: Vec<u8>,
    hole: u64,
}

impl ReadAt for Holed {
    fn read_at(&self, buffer: &mut [u8], offset: u64) -> std::io::Result<usize> {
        if offset / PAGE == self.hole {
            return Err(std::io::Error::from_raw_os_error(5));
        }
        // Up to the hole, as `/proc/PID/mem` reads up to a bad page.
        let stop = if offset / PAGE < self.hole {
            self.hole * PAGE
        } else {
            self.bytes.len() as u64
        };
        let from = usize::try_from(offset).unwrap();
        let to = usize::try_from(stop).unwrap().min(from + buffer.len());
        let part = &self.bytes[from..to];
        buffer[..part.len()].copy_from_slice(part);
        Ok(part.len())
    }
}

#[test]
fn a_secret_after_a_page_that_cannot_be_read_is_found() {
    let mut bytes = vec![0_u8; 3 * 4096];
    bytes[2 * 4096 + 10..2 * 4096 + 10 + SECRET.len()].copy_from_slice(SECRET);
    let mem = Holed { bytes, hole: 1 };
    let secrets = needles(&[SECRET.to_vec()], None);

    assert_eq!(search_region(&mem, 0, 3 * 4096, &secrets), [0]);
    assert_eq!(
        search_region(&mem, 0, 2 * 4096, &secrets),
        Vec::<usize>::new()
    );
}

#[test]
fn files_holding_a_secret_are_found() {
    let dir = std::env::temp_dir().join(format!("zbm-secret-search-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(
        dir.join("sub/key"),
        [b"x".as_slice(), SECRET, b"y"].concat(),
    )
    .unwrap();
    std::fs::write(dir.join("other"), b"nothing").unwrap();
    let mut findings = Findings::default();

    search_files(&dir, &needles(&[SECRET.to_vec()], None), &mut findings);

    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(findings.files, 2);
    assert_eq!(
        findings.found,
        [(0, format!("file {}", dir.join("sub/key").display()))]
    );
}

#[test]
fn a_process_holding_a_secret_is_found_in_memory_and_environment() {
    let text = String::from_utf8(SECRET.to_vec()).unwrap();
    let mut child = std::process::Command::new("sh")
        .args(["-c", "S=\"$ZBM_TEST_SECRET\"; read -r line"])
        .env("ZBM_TEST_SECRET", &text)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let mut findings = Findings::default();
    let other = b"zbm-never-anywhere-0451".to_vec();
    let pid = child.id();

    search_process(
        Path::new("/proc"),
        pid,
        &needles(&[other, SECRET.to_vec()], None),
        &mut findings,
    );

    drop(child.stdin.take());
    // Under `cargo test`, the supervisor's tests in the same process may
    // reap the child first.
    let _ = child.wait();
    let places: Vec<&str> = findings
        .found
        .iter()
        .map(|(_, place)| place.as_str())
        .collect();
    assert!(
        findings.found.iter().all(|(index, _)| *index == 1),
        "{places:?}"
    );
    assert!(
        places.iter().any(|place| place.ends_with(" environ")),
        "{places:?}"
    );
    assert!(
        findings.processes == 1 && findings.regions > 0,
        "{findings:?}"
    );
    assert!(
        places.iter().any(|place| place.contains(" memory ")),
        "{places:?}"
    );
}

#[test]
fn every_process_but_one_is_searched_in_a_proc_tree() {
    let proc = std::env::temp_dir().join(format!("zbm-secret-proc-{}", std::process::id()));
    let process = |pid: &str, memory: Option<&[u8]>| {
        let dir = proc.join(pid);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("comm"), "sh\n").unwrap();
        std::fs::write(dir.join("cmdline"), b"sh\0").unwrap();
        let maps = "0-40 r--p 0 0:0 0 [heap]\n1000-2000 r--p 0 0:0 0\n";
        std::fs::write(dir.join("maps"), maps).unwrap();
        if let Some(memory) = memory {
            std::fs::write(dir.join("mem"), memory).unwrap();
        }
    };
    let memory = [b"x".as_slice(), SECRET, b"y"].concat();
    process("7", Some(&memory));
    process("8", Some(&memory));
    process("9", None);
    std::fs::create_dir_all(proc.join("self")).unwrap();
    let mut findings = Findings::default();

    search_processes(&proc, 8, &needles(&[SECRET.to_vec()], None), &mut findings);

    std::fs::remove_dir_all(&proc).unwrap();
    assert_eq!(
        findings,
        Findings {
            processes: 1,
            regions: 2,
            files: 0,
            found: vec![(0, "process 7 (sh) memory 0-40 [heap]".to_owned())],
        }
    );
    search_files(&proc, &needles(&[SECRET.to_vec()], None), &mut findings);
    assert_eq!(findings.files, 0);
}

#[test]
fn the_report_names_each_place_and_the_protection() {
    let findings = Findings {
        processes: 3,
        regions: 40,
        files: 500,
        found: vec![(0, "file /run/x".to_owned())],
    };
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };

    assert_eq!(report(&findings, &mut io), 1);
    assert_eq!(report(&Findings::default(), &mut io), 0);

    let text = String::from_utf8(stdout).unwrap();
    let expected = "found secret 0 in file /run/x\n\
                    searched 3 processes, 40 mappings and 500 files; found 1; \
                    secrets kept as ";
    assert!(text.starts_with(expected), "{text}");
}

#[test]
fn a_secret_is_found_in_a_row_and_as_the_terminal_s_cells_hold_it() {
    let secrets = vec![b"ab".to_vec(), vec![0xff]];
    let found = needles(&secrets, Some(8));
    // Invalid UTF-8 has no cells.
    assert_eq!(found.len(), 3);
    assert_eq!(found[1].span(), 12);
    let mut cells = vec![0_u8; 32];
    cells[4..8].copy_from_slice(&u32::from('a').to_le_bytes());
    cells[12..16].copy_from_slice(&u32::from('b').to_le_bytes());
    assert_eq!(matches(&cells, &found), [0]);
    assert_eq!(matches(b"xxabxx", &found), [0]);
    assert_eq!(matches(b"a", &found), Vec::<usize>::new());
    assert_eq!(matches(&[0xff, 0], &found), [1]);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };
    assert_eq!(run(&["--cells"], &mut io), USAGE);
    const { assert!(zbm_console::CELL_SIZE >= 4) };
}
