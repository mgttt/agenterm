//! Zero-native-dependency host processor discovery.

pub use crate::contract::hardware::{ProcessorArchitecture, ProcessorFacts, ProcessorFeature};

pub const fn current_architecture() -> ProcessorArchitecture {
    #[cfg(target_arch = "x86_64")]
    return ProcessorArchitecture::X86_64;
    #[cfg(target_arch = "aarch64")]
    return ProcessorArchitecture::Aarch64;
    #[cfg(target_arch = "x86")]
    return ProcessorArchitecture::X86;
    #[cfg(target_arch = "arm")]
    return ProcessorArchitecture::Arm;
    #[cfg(target_arch = "riscv64")]
    return ProcessorArchitecture::RiscV64;
    #[cfg(target_arch = "riscv32")]
    return ProcessorArchitecture::RiscV32;
    #[cfg(target_arch = "xtensa")]
    return ProcessorArchitecture::Xtensa;
    #[cfg(target_arch = "wasm64")]
    return ProcessorArchitecture::Wasm64;
    #[cfg(target_arch = "wasm32")]
    return ProcessorArchitecture::Wasm32;
    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "x86",
        target_arch = "arm",
        target_arch = "riscv64",
        target_arch = "riscv32",
        target_arch = "xtensa",
        target_arch = "wasm64",
        target_arch = "wasm32"
    )))]
    return ProcessorArchitecture::Other(std::env::consts::ARCH);
}

pub fn processor_facts() -> ProcessorFacts {
    ProcessorFacts {
        architecture: current_architecture(),
        pointer_width: usize::BITS as u8,
        logical_processors: std::thread::available_parallelism().ok(),
        features: detected_features(),
    }
}

fn detected_features() -> Vec<ProcessorFeature> {
    let mut features = Vec::new();
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if std::is_x86_feature_detected!("sse2") {
            features.push(ProcessorFeature::X86Sse2);
        }
        if std::is_x86_feature_detected!("avx") {
            features.push(ProcessorFeature::X86Avx);
        }
        if std::is_x86_feature_detected!("avx2") {
            features.push(ProcessorFeature::X86Avx2);
        }
        if std::is_x86_feature_detected!("fma") {
            features.push(ProcessorFeature::X86Fma);
        }
    }
    #[cfg(target_arch = "aarch64")]
    if std::arch::is_aarch64_feature_detected!("neon") {
        features.push(ProcessorFeature::ArmNeon);
    }
    #[cfg(target_arch = "arm")]
    if std::arch::is_arm_feature_detected!("neon") {
        features.push(ProcessorFeature::ArmNeon);
    }
    features
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_match_the_compilation_target() {
        let facts = processor_facts();
        assert_eq!(facts.architecture, current_architecture());
        assert_eq!(facts.architecture.as_str(), std::env::consts::ARCH);
        assert_eq!(facts.pointer_width, usize::BITS as u8);
        assert!(facts.logical_processors.is_none_or(|count| count.get() > 0));
    }

    #[test]
    fn reported_features_are_unique_and_match_the_isa_family() {
        let facts = processor_facts();
        let unique = facts
            .features
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), facts.features.len());
        for feature in &facts.features {
            match feature {
                ProcessorFeature::X86Sse2
                | ProcessorFeature::X86Avx
                | ProcessorFeature::X86Avx2
                | ProcessorFeature::X86Fma => assert!(matches!(
                    facts.architecture,
                    ProcessorArchitecture::X86 | ProcessorArchitecture::X86_64
                )),
                ProcessorFeature::ArmNeon => assert!(matches!(
                    facts.architecture,
                    ProcessorArchitecture::Arm | ProcessorArchitecture::Aarch64
                )),
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn x86_64_baseline_includes_sse2() {
        assert!(processor_facts().supports(ProcessorFeature::X86Sse2));
    }
}
