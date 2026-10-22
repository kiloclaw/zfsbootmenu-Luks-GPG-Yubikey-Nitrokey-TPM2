//! Finding where a new `.cmdline` section of an EFI image can go, as
//! `find_cmdline_gap` does.

/// The address of the `.cmdline` section, as upper-case hex digits, and the
/// space up to the next section, from the section table that `objdump -h -w
/// <file>` prints.
///
/// The values follow the script's 64-bit shell arithmetic. Errors are the
/// messages the script prints.
pub fn cmdline_gap(objdump: &str, file: &str) -> Result<(String, i64), Vec<String>> {
    let mut header_seen = false;
    let mut cmdline = None;
    let mut others = Vec::new();
    for line in objdump.lines() {
        // `read -r idx name size vma rem`
        let mut fields = line.split([' ', '\t']).filter(|field| !field.is_empty());
        let idx = fields.next().unwrap_or_default();
        let name = fields.next().unwrap_or_default();
        let _size = fields.next();
        let vma = fields.next().unwrap_or_default();
        if idx.eq_ignore_ascii_case("idx") && vma.eq_ignore_ascii_case("vma") {
            header_seen = true;
            continue;
        }
        if !header_seen || idx.parse::<i64>().is_err() {
            continue;
        }
        let vma = vma.to_ascii_lowercase();
        let digits = vma.strip_prefix("0x").unwrap_or(&vma);
        let Some(address) = hex_value(digits) else {
            return Err(vec![format!("invalid VMA for section '{name}'")]);
        };
        if name.eq_ignore_ascii_case(".cmdline") {
            cmdline = Some(address);
        } else {
            others.push(address);
        }
    }
    let Some(cmdline) = cmdline else {
        return Err(vec![format!("file '{file}' contains no .cmdline section")]);
    };
    let gap = others
        .iter()
        .filter(|&&address| address > cmdline)
        .map(|&address| address.wrapping_sub(cmdline))
        .min();
    match gap {
        // `printf "%X %X\n"` prints the two's complement of negative values.
        Some(gap) => Ok((
            format!("{:X}", u64::from_ne_bytes(cmdline.to_ne_bytes())),
            gap,
        )),
        None => Err(vec!["unable to determine .cmdline gap size".to_owned()]),
    }
}

/// Whether a section of `size` bytes fits in `gap` bytes.
pub fn fits(size: usize, gap: i64) -> bool {
    i64::try_from(size).is_ok_and(|size| size <= gap)
}

/// The value of hex digits in 64-bit shell arithmetic (wrapping), or `None`
/// when they are not hex digits.
fn hex_value(digits: &str) -> Option<i64> {
    if digits.is_empty() {
        return None;
    }
    digits.chars().try_fold(0_i64, |value, digit| {
        let digit = i64::from(digit.to_digit(16)?);
        Some(value.wrapping_mul(16).wrapping_add(digit))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `objdump -w -h` of a ZFSBootMenu EFI bundle.
    const TABLE: &str = include_str!("../tests/fixtures/objdump-w-h.txt");

    #[test]
    fn finds_the_space_before_the_next_section() {
        assert_eq!(cmdline_gap(TABLE, "x"), Ok(("11000".to_owned(), 0x1000)));
        assert!(fits(0x1000, 0x1000));
        assert!(!fits(0x1001, 0x1000));
    }

    #[test]
    fn reports_missing_sections() {
        let without = TABLE.replace(".cmdline", ".other");
        assert_eq!(
            cmdline_gap(&without, "/tmp/x"),
            Err(vec![
                "file '/tmp/x' contains no .cmdline section".to_owned()
            ])
        );
        let last = TABLE
            .lines()
            .filter(|line| !line.contains(".initrd") && !line.contains(".linux"))
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        assert_eq!(
            cmdline_gap(&last, "x"),
            Err(vec!["unable to determine .cmdline gap size".to_owned()])
        );
        let bad = TABLE.replace("0000000000012000  0000000000012000", "00000000000zz000  0");
        assert_eq!(
            cmdline_gap(&bad, "x"),
            Err(vec!["invalid VMA for section '.initrd'".to_owned()])
        );
        // Lines before the header are ignored.
        assert!(cmdline_gap(&TABLE.replace("Idx", "Ind"), "x").is_err());
    }
}
