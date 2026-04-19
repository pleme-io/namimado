//! Provability test — every shipped normalize pack parses cleanly via
//! `nami_core::normalize::compile` and yields at least one spec.
//!
//! Fails at CI time (not at user install time) if anyone commits a
//! syntactically broken pack. Equivalent to the coherence checker
//! other pleme-io crates run over their DSL sources.

#![cfg(feature = "browser-core")]

use std::fs;
use std::path::PathBuf;

fn pack_dir() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest).join("examples").join("normalize-packs")
}

fn packs() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in fs::read_dir(pack_dir()).expect("packs dir exists") {
        let entry = entry.expect("entry readable");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("lisp") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("<unnamed>")
            .to_owned();
        let src = fs::read_to_string(&path).expect("pack readable");
        out.push((name, src));
    }
    out
}

#[test]
fn every_shipped_pack_compiles_cleanly() {
    let packs = packs();
    assert!(!packs.is_empty(), "no packs discovered under examples/normalize-packs/");
    for (name, src) in &packs {
        // Blocker packs compile through blocker::compile, not
        // normalize::compile. Route by filename prefix.
        if name.starts_with("blocker-") {
            let specs = nami_core::blocker::compile(src)
                .unwrap_or_else(|e| panic!("blocker pack {name} failed to compile: {e}"));
            assert!(!specs.is_empty(), "blocker pack {name} produced zero specs");
            continue;
        }
        match nami_core::normalize::compile(src) {
            Ok(specs) => {
                assert!(
                    !specs.is_empty(),
                    "pack {name} parsed but produced zero specs"
                );
                // Every rule must have non-empty name + selector + rename_to.
                for spec in &specs {
                    assert!(!spec.name.is_empty(), "pack {name}: empty rule name");
                    assert!(
                        !spec.selector.is_empty(),
                        "pack {name} rule {}: empty selector",
                        spec.name
                    );
                    assert!(
                        !spec.rename_to.is_empty(),
                        "pack {name} rule {}: empty rename_to",
                        spec.name
                    );
                }
            }
            Err(e) => panic!("pack {name} failed to compile: {e}"),
        }
    }
}

#[test]
fn pack_rule_names_are_unique_within_pack() {
    for (name, src) in packs() {
        let names: Vec<String> = if name.starts_with("blocker-") {
            nami_core::blocker::compile(&src)
                .expect("compile")
                .into_iter()
                .map(|s| s.name)
                .collect()
        } else {
            nami_core::normalize::compile(&src)
                .expect("compile")
                .into_iter()
                .map(|s| s.name)
                .collect()
        };
        let mut seen = std::collections::HashSet::new();
        for n in &names {
            assert!(
                seen.insert(n.clone()),
                "pack {name} has duplicate rule name: {n}",
            );
        }
    }
}

#[test]
fn emit_packs_target_tags_without_n_prefix() {
    // Outbound emit packs (file names ending in -emit.lisp) produce
    // framework-shaped tags — those tags should NOT start with `n-`
    // (which is the canonical prefix).
    for (name, src) in packs() {
        if !name.ends_with("-emit.lisp") {
            continue;
        }
        if name.starts_with("blocker-") {
            continue;
        }
        let specs = nami_core::normalize::compile(&src).expect("compile");
        for spec in &specs {
            assert!(
                !spec.rename_to.starts_with("n-"),
                "emit pack {name} rule {}: rename_to {:?} still uses canonical n-* prefix",
                spec.name,
                spec.rename_to
            );
            // And it should set at least one attribute — otherwise
            // the emit is indistinguishable from a retag.
            assert!(
                !spec.set_attrs.is_empty(),
                "emit pack {name} rule {}: no :set-attrs, what framework is this emitting?",
                spec.name
            );
        }
    }
}

#[test]
fn inbound_packs_target_canonical_n_prefix() {
    // Inbound fold packs (all packs NOT ending in -emit) produce n-*
    // canonical tags. This is the convention; breaking it silently
    // fragments the downstream tooling surface.
    for (name, src) in packs() {
        if name.ends_with("-emit.lisp") {
            continue;
        }
        if name.starts_with("blocker-") {
            continue;
        }
        let specs = nami_core::normalize::compile(&src).expect("compile");
        for spec in &specs {
            assert!(
                spec.rename_to.starts_with("n-"),
                "inbound pack {name} rule {}: rename_to {:?} should start with n-",
                spec.name,
                spec.rename_to
            );
        }
    }
}
