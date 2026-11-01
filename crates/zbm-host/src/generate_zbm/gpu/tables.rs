//! The GPUs `generate-zbm` knows: for each chip, its driver, its family, its
//! PCI device IDs and the firmware its driver loads for it.
//!
//! A driver lists every firmware file it may load (`modinfo -F firmware`),
//! but not which chip needs which, so the table names them by *stem*: the
//! start of a file name up to a `_` or a `.`. The stem `amdgpu/navi10` covers
//! `amdgpu/navi10_me.bin` and `amdgpu/navi10_vcn.bin`, but not
//! `amdgpu/navi12_me.bin`.
//!
//! AMD GPUs from Navi 3x on, and some APUs before them, name their firmware
//! after the versions of their IP blocks (`gc_12_0_0`, `psp_14_0_2`), which
//! the driver reads from the GPU itself. Which versions a chip has is not in
//! the kernel, so their entries are groups of kin chips with the files of all
//! of them; on a machine where `amdgpu` runs, [`super::host`] reads the exact
//! versions instead.

use super::Driver;
use super::ids::{
    AMD_BEIGE_GOBY, AMD_CARRIZO, AMD_DIMGREY_CAVEFISH, AMD_FIJI, AMD_NAVI10, AMD_NAVI12,
    AMD_NAVI14, AMD_NAVY_FLOUNDER, AMD_POLARIS10, AMD_POLARIS11, AMD_POLARIS12, AMD_RAVEN,
    AMD_RENOIR, AMD_SIENNA_CICHLID, AMD_STONEY, AMD_TONGA, AMD_VEGA10, AMD_VEGA12, AMD_VEGA20,
    AMD_VEGAM, AMD_YELLOW_CARP, INTEL_ADLP, INTEL_ADLS, INTEL_BMG, INTEL_BXT, INTEL_CML, INTEL_DG1,
    INTEL_DG2, INTEL_EHL, INTEL_GLK, INTEL_ICL, INTEL_KBL, INTEL_LNL, INTEL_MTL, INTEL_PTL,
    INTEL_RKL, INTEL_SKL, INTEL_TGL,
};

/// A chip, or a group of kin chips, and the firmware its driver loads.
#[derive(Debug)]
pub struct Chip {
    /// The driver of the chip.
    pub driver: Driver,
    /// The name to ask for it by, after the driver: `amdgpu:navi10`.
    pub name: &'static str,
    /// The family it belongs to, also a name to ask by: `amdgpu:gfx10`.
    pub family: &'static str,
    /// Its PCI device IDs, of the driver's vendor.
    pub devices: &'static [u16],
    /// The stems of its firmware files. The driver must list a file of the
    /// first, or the kernel does not know the chip.
    pub stems: &'static [&'static str],
    /// Whether its firmware is named after IP versions, which the build
    /// host can read from a running GPU.
    pub discovery: bool,
}

/// A chip whose firmware is named after it.
const fn named(
    driver: Driver,
    name: &'static str,
    family: &'static str,
    devices: &'static [u16],
    stems: &'static [&'static str],
) -> Chip {
    Chip {
        driver,
        name,
        family,
        devices,
        stems,
        discovery: false,
    }
}

/// A group of AMD chips whose firmware is named after IP versions.
const fn discovered(
    name: &'static str,
    family: &'static str,
    devices: &'static [u16],
    stems: &'static [&'static str],
) -> Chip {
    Chip {
        driver: Driver::Amdgpu,
        name,
        family,
        devices,
        stems,
        discovery: true,
    }
}

const AMD: Driver = Driver::Amdgpu;
const I915: Driver = Driver::I915;
const XE: Driver = Driver::Xe;

/// The chips, by driver and family.
pub static CHIPS: &[Chip] = &[
    // AMD GCN 3 and 4 (GFX8)
    named(AMD, "tonga", "gfx8", AMD_TONGA, &["amdgpu/tonga"]),
    named(AMD, "fiji", "gfx8", AMD_FIJI, &["amdgpu/fiji"]),
    named(AMD, "carrizo", "gfx8", AMD_CARRIZO, &["amdgpu/carrizo"]),
    named(AMD, "stoney", "gfx8", AMD_STONEY, &["amdgpu/stoney"]),
    named(
        AMD,
        "polaris10",
        "gfx8",
        AMD_POLARIS10,
        &["amdgpu/polaris10"],
    ),
    named(
        AMD,
        "polaris11",
        "gfx8",
        AMD_POLARIS11,
        &["amdgpu/polaris11"],
    ),
    named(
        AMD,
        "polaris12",
        "gfx8",
        AMD_POLARIS12,
        &["amdgpu/polaris12"],
    ),
    named(AMD, "vegam", "gfx8", AMD_VEGAM, &["amdgpu/vegam"]),
    // AMD Vega and its APUs (GFX9)
    named(AMD, "vega10", "gfx9", AMD_VEGA10, &["amdgpu/vega10"]),
    named(AMD, "vega12", "gfx9", AMD_VEGA12, &["amdgpu/vega12"]),
    named(AMD, "vega20", "gfx9", AMD_VEGA20, &["amdgpu/vega20"]),
    // Raven, Raven 2 and Picasso share their device IDs
    named(
        AMD,
        "raven",
        "gfx9",
        AMD_RAVEN,
        &["amdgpu/raven", "amdgpu/raven2", "amdgpu/picasso"],
    ),
    // Renoir, and Cezanne and Barcelo, whose firmware is Green Sardine's
    named(
        AMD,
        "renoir",
        "gfx9",
        AMD_RENOIR,
        &["amdgpu/renoir", "amdgpu/green_sardine"],
    ),
    // AMD RDNA 1 (GFX10)
    named(AMD, "navi10", "gfx10", AMD_NAVI10, &["amdgpu/navi10"]),
    named(AMD, "navi12", "gfx10", AMD_NAVI12, &["amdgpu/navi12"]),
    named(AMD, "navi14", "gfx10", AMD_NAVI14, &["amdgpu/navi14"]),
    // AMD RDNA 2 (GFX10.3)
    named(
        AMD,
        "navi21",
        "gfx10.3",
        AMD_SIENNA_CICHLID,
        &["amdgpu/sienna_cichlid"],
    ),
    named(
        AMD,
        "navi22",
        "gfx10.3",
        AMD_NAVY_FLOUNDER,
        &["amdgpu/navy_flounder"],
    ),
    named(
        AMD,
        "navi23",
        "gfx10.3",
        AMD_DIMGREY_CAVEFISH,
        &["amdgpu/dimgrey_cavefish"],
    ),
    named(
        AMD,
        "navi24",
        "gfx10.3",
        AMD_BEIGE_GOBY,
        &["amdgpu/beige_goby"],
    ),
    named(
        AMD,
        "rembrandt",
        "gfx10.3",
        AMD_YELLOW_CARP,
        &["amdgpu/yellow_carp"],
    ),
    // Raphael, Granite Ridge and Mendocino
    discovered(
        "raphael",
        "gfx10.3",
        &[0x13c0, 0x1506, 0x164e],
        &[
            "amdgpu/gc_10_3_6",
            "amdgpu/gc_10_3_7",
            "amdgpu/psp_13_0_5",
            "amdgpu/psp_13_0_8",
            "amdgpu/sdma_5_2_6",
            "amdgpu/sdma_5_2_7",
            "amdgpu/dcn_3_1_5",
            "amdgpu/dcn_3_1_6",
            "amdgpu/vcn_3_1_2",
        ],
    ),
    // AMD RDNA 3 (GFX11): Navi 31, 32 and 33
    discovered(
        "navi3x",
        "gfx11",
        &[
            0x73f0, 0x7448, 0x7449, 0x744a, 0x744b, 0x744c, 0x745e, 0x7460, 0x7461, 0x7470, 0x747e,
            0x7480, 0x7481, 0x7483, 0x7487, 0x7489, 0x748b, 0x7499, 0x749f,
        ],
        &[
            "amdgpu/gc_11_0_0",
            "amdgpu/gc_11_0_2",
            "amdgpu/gc_11_0_3",
            "amdgpu/psp_13_0_0",
            "amdgpu/psp_13_0_7",
            "amdgpu/psp_13_0_10",
            "amdgpu/smu_13_0_0",
            "amdgpu/smu_13_0_7",
            "amdgpu/smu_13_0_10",
            "amdgpu/sdma_6_0_0",
            "amdgpu/sdma_6_0_2",
            "amdgpu/sdma_6_0_3",
            "amdgpu/dcn_3_2_0",
            "amdgpu/dcn_3_2_1",
            "amdgpu/vcn_4_0_0",
            "amdgpu/vcn_4_0_4",
        ],
    ),
    // Phoenix and Hawk Point
    discovered(
        "phoenix",
        "gfx11",
        &[0x15bf, 0x15c8, 0x164f, 0x1900, 0x1901],
        &[
            "amdgpu/gc_11_0_1",
            "amdgpu/gc_11_0_4",
            "amdgpu/psp_13_0_4",
            "amdgpu/psp_13_0_11",
            "amdgpu/sdma_6_0_1",
            "amdgpu/dcn_3_1_4",
            "amdgpu/vcn_4_0_2",
        ],
    ),
    // AMD RDNA 3.5 (GFX11.5): Strix, Strix Halo and Krackan
    discovered(
        "strix",
        "gfx11.5",
        &[0x1114, 0x150e, 0x1586, 0x1902],
        &[
            "amdgpu/gc_11_5",
            "amdgpu/psp_14_0_0",
            "amdgpu/psp_14_0_1",
            "amdgpu/psp_14_0_4",
            "amdgpu/psp_14_0_5",
            "amdgpu/sdma_6_1",
            "amdgpu/dcn_3_5",
            "amdgpu/vcn_4_0_5",
            "amdgpu/vcn_4_0_6",
            "amdgpu/vpe_6_1",
        ],
    ),
    // AMD RDNA 4 (GFX12): Navi 44 and 48
    discovered(
        "navi4x",
        "gfx12",
        &[0x7550, 0x7551, 0x7590],
        &[
            "amdgpu/gc_12_0_0",
            "amdgpu/gc_12_0_1",
            "amdgpu/psp_14_0_2",
            "amdgpu/psp_14_0_3",
            "amdgpu/smu_14_0_2",
            "amdgpu/smu_14_0_3",
            "amdgpu/sdma_7_0_0",
            "amdgpu/sdma_7_0_1",
            "amdgpu/dcn_4_0_1",
            "amdgpu/vcn_5_0_0",
        ],
    ),
    // Intel Gen 9: the display firmware (DMC), GuC and HuC of each platform
    named(I915, "skl", "gen9", INTEL_SKL, &["i915/skl"]),
    named(I915, "kbl", "gen9", INTEL_KBL, &["i915/kbl"]),
    named(
        I915,
        "cml",
        "gen9",
        INTEL_CML,
        &["i915/kbl_dmc", "i915/cml"],
    ),
    named(I915, "glk", "gen9", INTEL_GLK, &["i915/glk"]),
    named(I915, "bxt", "gen9", INTEL_BXT, &["i915/bxt"]),
    // Intel Gen 11
    named(I915, "icl", "gen11", INTEL_ICL, &["i915/icl"]),
    named(
        I915,
        "ehl",
        "gen11",
        INTEL_EHL,
        &["i915/icl_dmc", "i915/ehl"],
    ),
    // Intel Gen 12 (Xe-LP and Xe-HPG)
    named(I915, "tgl", "gen12", INTEL_TGL, &["i915/tgl"]),
    named(
        I915,
        "rkl",
        "gen12",
        INTEL_RKL,
        &["i915/rkl_dmc", "i915/tgl_guc", "i915/tgl_huc"],
    ),
    named(
        I915,
        "adls",
        "gen12",
        INTEL_ADLS,
        &["i915/adls_dmc", "i915/tgl_guc", "i915/tgl_huc"],
    ),
    named(
        I915,
        "adlp",
        "gen12",
        INTEL_ADLP,
        &["i915/adlp_dmc", "i915/adlp_guc", "i915/tgl_huc"],
    ),
    named(I915, "dg1", "gen12", INTEL_DG1, &["i915/dg1"]),
    named(I915, "dg2", "gen12", INTEL_DG2, &["i915/dg2"]),
    // Intel Meteor Lake and Arrow Lake (Xe-LPG)
    named(I915, "mtl", "gen12.7", INTEL_MTL, &["i915/mtl"]),
    // Intel Xe2 and Xe3, which the xe driver drives; their display firmware
    // is in i915/
    named(XE, "lnl", "xe2", INTEL_LNL, &["xe/lnl", "i915/xe2lpd_dmc"]),
    named(XE, "bmg", "xe2", INTEL_BMG, &["xe/bmg", "i915/bmg_dmc"]),
    named(XE, "ptl", "xe3", INTEL_PTL, &["xe/ptl", "i915/xe3lpd"]),
];

/// The chip of a PCI device, if the table knows it.
pub fn chip_of(vendor: u16, device: u16) -> Option<&'static Chip> {
    CHIPS
        .iter()
        .find(|chip| chip.driver.vendor() == vendor && chip.devices.contains(&device))
}

/// The chips a name after `driver:` stands for: a chip or a family.
pub fn chips_named(driver: Driver, name: &str) -> Vec<&'static Chip> {
    CHIPS
        .iter()
        .filter(|chip| chip.driver == driver && (chip.name == name || chip.family == name))
        .collect()
}

/// The families of a driver, in table order, then its chips.
pub fn names(driver: Driver) -> (Vec<&'static str>, Vec<&'static str>) {
    let mut families: Vec<&'static str> = Vec::new();
    let mut chips = Vec::new();
    for chip in CHIPS.iter().filter(|chip| chip.driver == driver) {
        if !families.contains(&chip.family) {
            families.push(chip.family);
        }
        chips.push(chip.name);
    }
    (families, chips)
}
