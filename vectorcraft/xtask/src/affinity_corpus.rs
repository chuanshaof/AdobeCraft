//! `cargo xtask corpus --affinity`: public Affinity documents saved by Affinity 1.x to 3.x, from
//! their upstream repositories at pinned commits, each checked against `xtask/affinity-corpus.sha256`.
//! They land in `corpus/affinity/` (gitignored, never committed). `vectorcraft-affinity`'s
//! `real_files` test and the engine's `affinity_corpus` test use them when present.
//!
//! Only permissively licensed files (CC0, MIT, Apache-2.0) whose metadata holds no personal paths
//! are listed. Moving a pin: change the commit, re-run, review the manifest diff.

use std::path::Path;
use std::process::Command;

use crate::{root, run, sha256};

/// (repository, commit, licence, [(path in the repository, file name in corpus/affinity)]).
type Source = (&'static str, &'static str, &'static str, &'static [(&'static str, &'static str)]);

const SOURCES: &[Source] = &[
    (
        "samuel-etver/vector-art",
        "255f8add3c8f0740196e22bd59502b811b532f0b",
        "CC0-1.0",
        &[
            ("simple/mexican-guy.af", "affinity3-mexican-guy.af"),
            ("simple/mexican-man.af", "affinity3-mexican-man.af"),
            ("simple/mexican-woman.af", "affinity3-mexican-woman.af"),
            ("simple/playing-cards.af", "affinity3-playing-cards.af"),
            ("simple/beer.afdesign", "designer-beer.afdesign"),
            ("simple/cactus.afdesign", "designer-cactus.afdesign"),
            ("simple/car.afdesign", "designer-car.afdesign"),
            ("simple/flowers.afdesign", "designer-flowers.afdesign"),
            ("simple/lion-track.afdesign", "designer-lion-track.afdesign"),
        ],
    ),
    (
        "NickBeeuwsaert/AFDesignLoad",
        "a18dd50a7079fb861a28835eeefa0d015f03f4a1",
        "MIT",
        &[
            ("testDesigns/color.afdesign", "afdesignload-color.afdesign"),
            ("testDesigns/layer_mode.afdesign", "afdesignload-layer_mode.afdesign"),
            ("testDesigns/layer_test.afdesign", "afdesignload-layer_test.afdesign"),
            ("testDesigns/margins.afdesign", "afdesignload-margins.afdesign"),
            ("testDesigns/raster_test.afdesign", "afdesignload-raster_test.afdesign"),
            ("testDesigns/revision_test.afdesign", "afdesignload-revision_test.afdesign"),
            ("testDesigns/shape_test.afdesign", "afdesignload-shape_test.afdesign"),
            ("testDesigns/slice_test.afdesign", "afdesignload-slice_test.afdesign"),
            ("testDesigns/test_path.afdesign", "afdesignload-test_path.afdesign"),
        ],
    ),
    (
        "Jac21/Branding",
        "57ae3f45bf4637f6927c0b07de25b922c55f944f",
        "MIT",
        &[
            ("Logos/JC/DesignerFiles/Affinity/SimpleLogo.afdesign", "jac21-SimpleLogo.afdesign"),
            ("Logos/JC/DesignerFiles/Affinity/SimpleLogoBanner.afdesign", "jac21-SimpleLogoBanner.afdesign"),
        ],
    ),
    (
        "eviltwo/AssetStoreTemplate",
        "f671981ee89d15526285b3cc87b4393d3defe644",
        "MIT",
        &[("AssetStoreTemplate/AssetStore.aftemplate", "eviltwo-AssetStore.aftemplate")],
    ),
    (
        "square/leakcanary",
        "00880a8fb66eb4046a0124bff8303a04874bc07e",
        "Apache-2.0",
        &[("docs/assets/vector_icon.afdesign", "leakcanary-vector_icon.afdesign")],
    ),
];

const MANIFEST: &str = "xtask/affinity-corpus.sha256";

pub fn fetch() -> Result<(), String> {
    let root = root();
    let manifest = std::fs::read_to_string(root.join(MANIFEST)).map_err(|e| format!("{MANIFEST}: {e}"))?;
    let expected: Vec<(&str, &str)> = manifest.lines().filter_map(|l| l.split_once("  ")).collect();
    let dest = root.join("corpus").join("affinity");
    std::fs::create_dir_all(&dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    let mut sources = String::from(
        "# Public Affinity documents\n\nFetched by `cargo xtask corpus --affinity` and checked against `xtask/affinity-corpus.sha256`.\nGitignored; never commit these files.\n\n| File | Source | Licence |\n|---|---|---|\n",
    );
    let mut count = 0;
    for (repo, commit, licence, files) in SOURCES {
        for (path, name) in *files {
            let sha = expected.iter().find(|(_, n)| n == name).map(|(s, _)| *s).ok_or_else(|| format!("{name} is missing from {MANIFEST}"))?;
            let out = dest.join(name);
            if !verified(&out, sha) {
                let url = format!("https://raw.githubusercontent.com/{repo}/{commit}/{path}");
                let mut curl = Command::new("curl");
                curl.args(["-fsSL", "--retry", "3", "-A", "VectorCraft-dev", "-o"]).arg(&out).arg(&url);
                run(curl, &format!("curl {url}"))?;
                if !verified(&out, sha) {
                    let _ = std::fs::remove_file(&out);
                    return Err(format!("{name}: sha256 does not match {MANIFEST}"));
                }
            }
            sources.push_str(&format!("| {name} | https://github.com/{repo}/blob/{commit}/{path} | {licence} |\n"));
            count += 1;
        }
    }
    std::fs::write(dest.join("SOURCES.md"), sources).map_err(|e| format!("SOURCES.md: {e}"))?;
    println!("Affinity: {count} documents verified in {}", dest.display());
    Ok(())
}

fn verified(path: &Path, sha: &str) -> bool {
    std::fs::read(path).is_ok_and(|b| sha256::hex(&b) == sha)
}
