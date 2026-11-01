//! The host tools of ZFSBootMenu (ROADMAP.md DM10, ADR 0013), one crate in
//! the orchestration layer with two programs:
//!
//! - [`generate_zbm`]: `generate-zbm`, which builds images;
//! - [`kcl`]: `zbm-kcl`, which reads and writes the kernel command line of
//!   boot environments and EFI images;
//!
//! and [`image`], the plan of an image (SB8, ADR 0006), which the dracut
//! module and the mkinitcpio hook install (`zbm image plan`).

pub mod files;
pub mod generate_zbm;
pub mod image;
pub mod kcl;
pub mod options;
pub mod pe;
