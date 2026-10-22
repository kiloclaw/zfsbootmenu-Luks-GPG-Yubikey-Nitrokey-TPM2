//! Addresses as offsets from the binaries they are in, from
//! `/proc/self/maps`.

use super::Frame;

/// One mapping of `/proc/self/maps` that has a file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mapping {
    /// The first address.
    pub start: u64,
    /// The address after the last.
    pub end: u64,
    /// The offset in the file of the first address.
    pub offset: u64,
    /// The file.
    pub path: String,
}

/// The mappings of files in the text of `/proc/PID/maps`; lines without a
/// file (the heap, the stack, anonymous memory) and lines that cannot be read
/// are left out.
///
/// ```
/// use zbm_trace::crash::parse_maps;
///
/// let maps = parse_maps(
///     "55d0c0a00000-55d0c0a40000 r--p 00000000 00:11 12 /libexec/zbm\n\
///      7ffd5c000000-7ffd5c021000 rw-p 00000000 00:00 0  [stack]\n",
/// );
/// assert_eq!(maps.len(), 1);
/// assert_eq!(maps[0].path, "/libexec/zbm");
/// ```
pub fn parse_maps(text: &str) -> Vec<Mapping> {
    text.lines().filter_map(parse_line).collect()
}

/// One line of `/proc/PID/maps`.
fn parse_line(line: &str) -> Option<Mapping> {
    let mut fields = line.split_whitespace();
    let (start, end) = fields.next()?.split_once('-')?;
    let _permissions = fields.next()?;
    let offset = fields.next()?;
    let _device = fields.next()?;
    let _inode = fields.next()?;
    let path: Vec<&str> = fields.collect();
    let path = path.join(" ");
    if !path.starts_with('/') {
        return None;
    }
    Some(Mapping {
        start: u64::from_str_radix(start, 16).ok()?,
        end: u64::from_str_radix(end, 16).ok()?,
        offset: u64::from_str_radix(offset, 16).ok()?,
        path,
    })
}

/// The frame of `address`: its offset from the load address of the binary
/// whose mapping holds it, the lowest address any mapping of that file
/// starts at (less the file offset there), so that the offset is the address
/// the binary was linked for, whatever address space randomisation chose.
/// An address outside every file keeps its value, with the module `?`.
pub fn locate(address: u64, maps: &[Mapping]) -> Frame {
    let holder = maps
        .iter()
        .find(|mapping| (mapping.start..mapping.end).contains(&address));
    let Some(holder) = holder else {
        return Frame {
            offset: address,
            module: "?".to_owned(),
        };
    };
    let base = maps
        .iter()
        .filter(|mapping| mapping.path == holder.path)
        .map(|mapping| mapping.start.saturating_sub(mapping.offset))
        .min()
        .unwrap_or(holder.start);
    Frame {
        offset: address.saturating_sub(base),
        module: holder.path.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const MAPS: &str = "\
5600aa000000-5600aa040000 r--p 00000000 00:11 12 /usr/libexec/zfsbootmenu/zbm
5600aa040000-5600aa200000 r-xp 00040000 00:11 12 /usr/libexec/zfsbootmenu/zbm
5600aa200000-5600aa230000 r--p 00200000 00:11 12 /usr/libexec/zfsbootmenu/zbm
5600ab000000-5600ab021000 rw-p 00000000 00:00 0  [heap]
7f0011000000-7f0011030000 r-xp 00028000 08:01 99 /lib/x86_64-linux-gnu/libc.so.6
7f0011100000-7f0011101000 r--p 00000000 08:01 7  /usr/lib/a file with spaces.so
";

    #[test]
    fn only_mappings_of_files_are_kept() {
        let maps = parse_maps(MAPS);

        let paths: Vec<&str> = maps.iter().map(|mapping| mapping.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/usr/libexec/zfsbootmenu/zbm",
                "/usr/libexec/zfsbootmenu/zbm",
                "/usr/libexec/zfsbootmenu/zbm",
                "/lib/x86_64-linux-gnu/libc.so.6",
                "/usr/lib/a file with spaces.so",
            ]
        );
        assert_eq!(parse_maps("junk\n1-2 r 0 0 0\nzz-1 r--p 0 0 0 /x\n"), []);
    }

    #[test]
    fn addresses_become_offsets_from_the_load_address() {
        let maps = parse_maps(MAPS);
        let frame = |address| locate(address, &maps);

        assert_eq!(
            frame(0x5600_aa04_1234),
            Frame {
                offset: 0x41234,
                module: "/usr/libexec/zfsbootmenu/zbm".to_owned()
            }
        );
        // The first mapping of libc in the list is not at offset 0.
        assert_eq!(frame(0x7f00_1100_0010).offset, 0x28010);
        assert_eq!(
            frame(0x1234),
            Frame {
                offset: 0x1234,
                module: "?".to_owned()
            }
        );
    }
}
