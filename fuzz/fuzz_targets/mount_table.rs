//! The kernel's table of mounts (`/proc/self/mountinfo`), whose mount
//! points and sources hold the names that whoever made a pool chooses (A3,
//! inputs I3 and I5 of docs/dev/threat-model.md), and the mounts that a
//! chroot's code makes. Reading it may not panic or hang, and whether one
//! mount is another mount of the same filesystem is the same both ways and
//! never true of a mount itself. Only the first 64 mounts are compared, so
//! that the work per input stays bounded.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zbm_tools::mount::table::{Listed, mounts};

fuzz_target!(|data: &[u8]| {
    let listed: Vec<Listed> = mounts(data).collect();
    let first = listed.get(..64).unwrap_or(&listed);
    for one in first {
        assert!(!one.is_mounted_as(one));
        for other in first {
            assert_eq!(one.is_mounted_as(other), other.is_mounted_as(one));
        }
    }
});
