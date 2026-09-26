use crate::risk::pulseguard::pqc::signer::{IntegrityManifest, PqcSigner};
use log::info;
use std::fs;
use std::path::Path;

pub fn generate_manifest(
    signer: &PqcSigner,
    paths: &[&str],
) -> Result<IntegrityManifest, Box<dyn std::error::Error>> {
    let mut collected = Vec::new();
    for p in paths {
        let bytes = fs::read(p)?;
        collected.push((*p, bytes));
    }
    // Build manifest directly from collected slice (path,&[u8])
    let pairs: Vec<(&str, Vec<u8>)> = collected.into_iter().collect();
    let manifest = signer.build_manifest(
        &pairs
            .iter()
            .map(|(p, b)| (*p, b.clone()))
            .collect::<Vec<_>>(),
    )?;
    info!(
        "Integrity manifest generated with {} items",
        manifest.items.len()
    );
    Ok(manifest)
}

pub fn write_manifest<P: AsRef<Path>>(
    manifest: &IntegrityManifest,
    path: P,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(manifest)?;
    fs::write(path, json)?;
    Ok(())
}
