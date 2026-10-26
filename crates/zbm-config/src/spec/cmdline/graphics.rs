//! The graphics drivers of the image at boot (ROADMAP.md #17).

use super::{Doc, OptionSpec, Reader, Source, Value};

/// `zbm.gpu`: the GPU the graphics drivers drive, or none.
pub static GPU: OptionSpec = OptionSpec {
    name: "zbm.gpu",
    aliases: &[],
    source: Source::KernelCommandLine,
    value: Value::Text,
    default: None,
    summary: "The GPU the image's graphics drivers drive: a PCI address, amd, intel or off",
    reader: Reader::Rust,
    doc: Doc::Entry {
        heading: "**zbm.gpu=0000:03:00.0**",
        body: GPU_DOC,
    },
    deprecation: None,
    examples: &["zbm.gpu=0000:03:00.0", "zbm.gpu=amd", "zbm.gpu=off"],
};

const GPU_DOC: &str = r"
In images with graphics drivers (*Kernel.GraphicsDrivers* of **generate-zbm**), which only the
*rust* runtime takes, choose the GPU they drive. The drivers load before anything is asked, so the
display does not change mode under a passphrase prompt, and the console on the display
(**zbm.console=drm**) draws on that GPU's first connected display. A PCI address, as **lspci -D**
shows it (*0000:03:00.0*, or *03:00.0* for domain 0), names one GPU; **amd** or **intel** names
the GPUs of a vendor. The other GPUs are left alone: no driver takes them, and they keep what the
firmware showed. Without this option, the drivers take every GPU they can, and the console draws on
the first with a connected display. **zbm.gpu=off** loads no graphics driver for this boot, for a
GPU that does not come back after ZFSBootMenu: the console stays on the firmware's framebuffer.
";
