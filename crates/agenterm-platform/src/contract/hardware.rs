//! Product-neutral host processor facts.

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ProcessorArchitecture {
    X86_64,
    Aarch64,
    X86,
    Arm,
    RiscV64,
    RiscV32,
    Xtensa,
    Wasm64,
    Wasm32,
    Other(&'static str),
}

impl ProcessorArchitecture {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
            Self::X86 => "x86",
            Self::Arm => "arm",
            Self::RiscV64 => "riscv64",
            Self::RiscV32 => "riscv32",
            Self::Xtensa => "xtensa",
            Self::Wasm64 => "wasm64",
            Self::Wasm32 => "wasm32",
            Self::Other(name) => name,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ProcessorFeature {
    X86Sse2,
    X86Avx,
    X86Avx2,
    X86Fma,
    ArmNeon,
}

impl ProcessorFeature {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X86Sse2 => "x86.sse2",
            Self::X86Avx => "x86.avx",
            Self::X86Avx2 => "x86.avx2",
            Self::X86Fma => "x86.fma",
            Self::ArmNeon => "arm.neon",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessorFacts {
    pub architecture: ProcessorArchitecture,
    pub pointer_width: u8,
    pub logical_processors: Option<std::num::NonZeroUsize>,
    pub features: Vec<ProcessorFeature>,
}

impl ProcessorFacts {
    pub fn supports(&self, feature: ProcessorFeature) -> bool {
        self.features.contains(&feature)
    }
}
