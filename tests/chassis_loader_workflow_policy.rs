//! Policy lock for the typed six-cell Chassis-L1 delivery chain.

const CI: &str = include_str!("../.github/workflows/ci-chassis.yml");
const CANDIDATE: &str = include_str!("../.github/workflows/candidate.yml");
const PROMOTION: &str = include_str!("../.github/workflows/release.yml");
const STAGE: &str = include_str!("../scripts/chassis-stage-l1-loader.py");
const PACK: &str = include_str!("../scripts/chassis-candidate-pack.py");
const WORKBENCH_IMAGE: &str = include_str!("../src/frontend/chassis_image.rs");

const CELLS: [(&str, &str, &str); 6] = [
    ("windows-x86_64", "win-x86_64", "x86_64-pc-windows-msvc"),
    ("windows-aarch64", "win-aarch64", "aarch64-pc-windows-msvc"),
    ("linux-x86_64", "lnx-x86_64", "x86_64-unknown-linux-gnu"),
    ("linux-aarch64", "lnx-aarch64", "aarch64-unknown-linux-gnu"),
    ("macos-x86_64", "osx-x86_64", "x86_64-apple-darwin"),
    ("macos-aarch64", "osx-aarch64", "aarch64-apple-darwin"),
];

#[derive(Clone)]
struct Sources {
    ci: String,
    candidate: String,
    promotion: String,
    stage: String,
    pack: String,
    workbench_image: String,
}

type MutationCase = (&'static str, &'static str, fn(&mut Sources));

impl Sources {
    fn repository() -> Self {
        Self {
            ci: CI.replace("\r\n", "\n"),
            candidate: CANDIDATE.replace("\r\n", "\n"),
            promotion: PROMOTION.replace("\r\n", "\n"),
            stage: STAGE.replace("\r\n", "\n"),
            pack: PACK.replace("\r\n", "\n"),
            workbench_image: WORKBENCH_IMAGE.replace("\r\n", "\n"),
        }
    }
}

fn named_step<'a>(workflow: &'a str, name: &str) -> Result<&'a str, String> {
    let marker = format!("      - name: {name}\n");
    let start = workflow
        .find(&marker)
        .ok_or_else(|| format!("missing step: {name}"))?;
    let body = &workflow[start + marker.len()..];
    let end = body.find("\n      - ").unwrap_or(body.len());
    Ok(&body[..end])
}

fn require(source: &str, needle: &str, label: &str) -> Result<(), String> {
    if source.contains(needle) {
        Ok(())
    } else {
        Err(label.to_owned())
    }
}

fn validate_chain(s: &Sources) -> Result<(), String> {
    let ci_matrix =
        s.ci.split("    runs-on: ${{ matrix.runner }}")
            .next()
            .ok_or_else(|| "CI six-cell matrix".to_owned())?;
    let ci_native = named_step(&s.ci, "Test chassis on native cell")?;
    let ci_cross = named_step(&s.ci, "Check chassis on cross cell")?;
    for (_, cell, target) in CELLS {
        require(ci_matrix, &format!("cell: {cell}"), "CI six-cell matrix")?;
        require(
            ci_matrix,
            &format!("target: {target}"),
            "CI six-cell matrix",
        )?;
    }
    for step in [ci_native, ci_cross] {
        require(
            step,
            "-p agenterm-chassis --features loader",
            "CI loader feature",
        )?;
    }

    let build = named_step(&s.candidate, "Build thin Chassis-L1 loader")?;
    require(
        build,
        "case \"${{ matrix.platform_id }}\" in",
        "Candidate six-cell build",
    )?;
    if build.matches("--features loader").count() != 6
        || build.matches("--bin agenterm-chassis-loader").count() != 6
    {
        return Err("Candidate six-cell build".to_owned());
    }
    for (platform, _, target) in CELLS {
        require(build, &format!("{platform})"), "Candidate six-cell build")?;
        if platform != "windows-x86_64" {
            require(
                build,
                &format!("--target {target}"),
                "Candidate six-cell build",
            )?;
        }
    }
    require(
        build,
        "cp \"$loader\" target/chassis-l1-loader",
        "Candidate thin loader output",
    )?;

    let stage = named_step(&s.candidate, "Stage flat candidate part")?;
    for (platform, cell, _) in CELLS.into_iter().take(4) {
        require(
            stage,
            &format!("{platform})"),
            "Candidate platform-to-cell map",
        )?;
        require(
            stage,
            &format!("cell=\"{cell}\""),
            "Candidate platform-to-cell map",
        )?;
    }
    for mac_mapping in [
        "macos-aarch64|macos-x86_64)",
        "arch=\"${PLATFORM_ID#macos-}\"",
        "cell=\"osx-$arch\"",
    ] {
        require(stage, mac_mapping, "Candidate platform-to-cell map")?;
    }
    for argument in [
        "python3 scripts/chassis-stage-l1-loader.py",
        "--loader target/chassis-l1-loader",
        "--cell \"$cell\"",
        "--source-sha \"$SOURCE_SHA\"",
        "--out \"$stage\"",
    ] {
        require(stage, argument, "Candidate typed staging")?;
    }
    for field in [
        "\"kind\": \"agenterm-chassis-l1-loader\"",
        "\"cell\": args.cell",
        "\"source_sha\": source_sha",
        "\"bytes\": size",
        "\"sha256\": sha256_file(loader)",
        "\"max_bytes\": MAX_LOADER_BYTES",
        "root / \"loader.json\"",
    ] {
        require(&s.stage, field, "typed loader descriptor")?;
    }

    let compose = named_step(
        &s.candidate,
        "Compose Candidate chassis product without Cargo",
    )?;
    require(
        compose,
        "scripts/chassis-candidate-pack.py",
        "Candidate typed pack",
    )?;
    for typed_input in [
        "root = candidate_input / \"chassis-l1\" / cell",
        "loader = root / \"loader\"",
        "descriptor_path = root / \"loader.json\"",
        "source = one_loader(candidate_input, version, source_sha, cell)",
        "shutil.copyfile(source, loader)",
    ] {
        require(&s.pack, typed_input, "pack consumes typed loader")?;
    }
    require(
        &s.pack,
        "typed Candidate L1 loader is missing for {cell}",
        "pack fail-closed typed input",
    )?;

    for install_contract in [
        "if !root.is_dir()",
        "chassis image is not an installed directory",
        "extract a Candidate .tgz before launch",
        "root.join(\"l1\").join(native_cell).join(\"loader\")",
        "agenterm_platform::chassis_loader::validate_executable(loader, &bytes)",
    ] {
        require(
            &s.workbench_image,
            install_contract,
            "workbench installed typed loader",
        )?;
    }

    require(
        &s.promotion,
        "  workflow_dispatch:\n",
        "Promotion human trigger",
    )?;
    if s.promotion.contains("\n  push:") || s.promotion.contains("\n  workflow_run:") {
        return Err("Promotion human trigger".to_owned());
    }
    let resolve = named_step(&s.promotion, "Resolve exact candidate run")?;
    require(
        resolve,
        "$(jq -r .path <<<\"$run\")",
        "Promotion Candidate binding",
    )?;
    require(
        resolve,
        "\".github/workflows/candidate.yml\"",
        "Promotion Candidate binding",
    )?;
    require(
        &s.promotion,
        "[[ \"$CONFIRMATION\" == \"publish-$tag\" ]]",
        "Promotion human confirmation",
    )?;
    Ok(())
}

fn remove_once(source: &mut String, needle: &str) {
    assert!(source.contains(needle), "mutation needle drifted: {needle}");
    *source = source.replacen(needle, "", 1);
}

#[test]
fn repository_keeps_the_complete_typed_loader_delivery_chain() {
    validate_chain(&Sources::repository()).expect("typed loader delivery policy");
}

#[test]
fn deleting_any_delivery_link_is_rejected() {
    let cases: [MutationCase; 8] = [
        ("CI loader feature", "CI loader feature", |s| {
            remove_once(&mut s.ci, " --features loader")
        }),
        (
            "Candidate six-cell build",
            "Candidate six-cell build",
            |s| remove_once(&mut s.candidate, "--bin agenterm-chassis-loader"),
        ),
        ("Candidate typed staging", "Candidate typed staging", |s| {
            remove_once(&mut s.candidate, "--loader target/chassis-l1-loader")
        }),
        ("typed loader descriptor", "typed loader descriptor", |s| {
            remove_once(&mut s.stage, "root / \"loader.json\"")
        }),
        (
            "pack consumes typed loader",
            "pack consumes typed loader",
            |s| remove_once(&mut s.pack, "descriptor_path = root / \"loader.json\""),
        ),
        (
            "workbench installed typed loader",
            "workbench installed typed loader",
            |s| {
                remove_once(
                    &mut s.workbench_image,
                    "agenterm_platform::chassis_loader::validate_executable(loader, &bytes)",
                )
            },
        ),
        ("Promotion human trigger", "Promotion human trigger", |s| {
            remove_once(&mut s.promotion, "  workflow_dispatch:\n")
        }),
        (
            "Promotion human confirmation",
            "Promotion human confirmation",
            |s| {
                remove_once(
                    &mut s.promotion,
                    "[[ \"$CONFIRMATION\" == \"publish-$tag\" ]]",
                )
            },
        ),
    ];

    for (case, expected, mutate) in cases {
        let mut sources = Sources::repository();
        mutate(&mut sources);
        let error = validate_chain(&sources).expect_err(case);
        assert!(
            error.contains(expected),
            "{case}: expected {expected:?}, got {error:?}"
        );
    }
}

#[test]
fn fat_archives_cannot_substitute_for_typed_l1_inputs() {
    let mut sources = Sources::repository();
    remove_once(
        &mut sources.pack,
        "root = candidate_input / \"chassis-l1\" / cell",
    );
    sources
        .pack
        .push_str("\nloader = candidate_input / format!(\"agenterm-{version}-{cell}.tar.gz\")\n");

    let error = validate_chain(&sources).expect_err("fat archive substitution");
    assert_eq!(error, "pack consumes typed loader");
}
