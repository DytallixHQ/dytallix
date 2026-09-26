use dytallix_pqc::PQCManager;
use std::env;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pqc_manager = PQCManager::new()?;

    let output_file = env::args()
        .nth(1)
        .unwrap_or_else(|| "pqc_keys.json".to_string());
    pqc_manager.save_to_file(&output_file)?;

    println!("Keys generated and saved to: {output_file}");
    Ok(())
}
