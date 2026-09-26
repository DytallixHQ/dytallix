//! Init command implementation.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use dytallix_core::address::DAddr;
use dytallix_core::keypair::DytallixKeypair;
use dytallix_sdk::Balance;

use crate::commands::{
    display_path, faucet_balance, faucet_balance_timeout, faucet_request, format_number,
    humanize_sdk_error, load_or_create_keystore, short_address,
};
use crate::output;

/// Runs the `init` command.
pub async fn run() -> Result<()> {
    println!("Dytallix Testnet — Initializing");
    println!();
    let mut services = RealInitServices;
    run_with_services(&mut services, |line| println!("{line}")).await?;
    Ok(())
}

async fn run_with_services<S, F>(services: &mut S, mut emit: F) -> Result<Duration>
where
    S: InitServices,
    F: FnMut(String),
{
    let overall = Instant::now();

    let keypair = services.generate_keypair().await?;
    let keypair_elapsed = overall.elapsed();
    emit(format!(
        "✓ ML-DSA-65 keypair generated          [{:.1}s]",
        keypair_elapsed.as_secs_f64()
    ));

    let address = DAddr::from_public_key(keypair.public_key())?;
    emit(format!(
        "✓ D-Addr derived: {}    [{:.1}s]",
        short_address(&address),
        overall.elapsed().as_secs_f64()
    ));

    let keystore_path = services.save_keypair(&keypair).await?;
    emit(format!(
        "✓ Keystore saved: {}",
        display_path(&keystore_path)
    ));

    services.submit_faucet(&address).await?;
    emit(format!(
        "✓ Faucet request submitted             [{:.1}s]",
        overall.elapsed().as_secs_f64()
    ));

    let deadline = Instant::now() + Duration::from_secs(45);
    let balance = loop {
        let balance = services.poll_balance(&address).await?;
        // DRT is the faucet-obtainable token that pays transaction gas. DGT is
        // the governance token and is NOT dispensed by the faucet, so its
        // balance stays 0 for a freshly-funded wallet. Confirm funding on DRT;
        // requiring DGT here could never succeed and always timed out at 45s.
        if balance.drt > 0 {
            break balance;
        }
        if Instant::now() >= deadline {
            return Err(faucet_balance_timeout(&address));
        }
        services.wait(Duration::from_secs(2)).await;
    };

    let received_elapsed = overall.elapsed();
    emit(format!(
        "✓ DRT received: {} DRT            [{:.1}s]",
        format_number(balance.drt),
        received_elapsed.as_secs_f64()
    ));
    // The faucet does not dispense DGT, so only surface it when actually present
    // (e.g. a pre-funded governance wallet) instead of printing "0 DGT".
    if balance.dgt > 0 {
        emit(format!(
            "✓ DGT received: {} DGT             [{:.1}s]",
            format_number(balance.dgt),
            received_elapsed.as_secs_f64()
        ));
    }
    emit(String::new());
    emit("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_owned());
    emit("  Milestone 1 complete: funded wallet".to_owned());
    emit(format!(
        "  Elapsed: {:.1}s",
        overall.elapsed().as_secs_f64()
    ));
    emit("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_owned());
    emit(String::new());

    output::testnet_warning();
    println!();
    output::divider();
    println!("  Next: send your first transaction");
    output::divider();
    println!();
    println!("  dytallix wallet create --name recipient");
    println!("  dytallix wallet switch recipient");
    println!("  dytallix wallet info");
    println!("  dytallix wallet switch default");
    println!("  dytallix send <recipient-daddr> 100");
    println!();
    println!("  Create a separate recipient wallet first.");
    println!("  Do not self-send the address from dytallix init.");
    println!("  Gas paid in DRT automatically.");
    println!("  Fee breakdown shown before confirmation.");
    println!("  Run it now to hit Milestone 2.");
    println!();
    output::divider();
    println!("  Next: deploy your first contract");
    output::divider();
    println!();
    println!("  dytallix contract deploy <path-to-your-contract.wasm>");
    println!();
    println!("  Build a contract first, then run the deploy command.");
    println!("  Gas paid in DRT automatically.");
    println!("  Fee breakdown shown before confirmation.");
    println!("  See docs/getting-started.md for a");
    println!("  complete walkthrough to Milestone 3.");

    Ok(overall.elapsed())
}

trait InitServices {
    async fn generate_keypair(&mut self) -> Result<DytallixKeypair>;
    async fn save_keypair(&mut self, keypair: &DytallixKeypair) -> Result<std::path::PathBuf>;
    async fn submit_faucet(&mut self, address: &DAddr) -> Result<()>;
    async fn poll_balance(&mut self, address: &DAddr) -> Result<Balance>;
    async fn wait(&mut self, duration: Duration);
}

struct RealInitServices;

impl InitServices for RealInitServices {
    async fn generate_keypair(&mut self) -> Result<DytallixKeypair> {
        Ok(DytallixKeypair::generate())
    }

    async fn save_keypair(&mut self, keypair: &DytallixKeypair) -> Result<std::path::PathBuf> {
        let mut keystore = load_or_create_keystore()?;
        let name = if keystore.list().is_empty() {
            "default".to_owned()
        } else {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or(0);
            format!("wallet-{timestamp}")
        };
        keystore
            .add_keypair(keypair, &name)
            .map_err(humanize_sdk_error)?;
        keystore.set_active(&name).map_err(humanize_sdk_error)?;
        keystore.save().map_err(humanize_sdk_error)?;
        Ok(dytallix_sdk::keystore::Keystore::default_path())
    }

    async fn submit_faucet(&mut self, address: &DAddr) -> Result<()> {
        faucet_request(address, "both").await?;
        Ok(())
    }

    async fn poll_balance(&mut self, address: &DAddr) -> Result<Balance> {
        faucet_balance(address).await
    }

    async fn wait(&mut self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::{run_with_services, InitServices};
    use dytallix_core::address::DAddr;
    use dytallix_core::keypair::DytallixKeypair;
    use dytallix_sdk::Balance;

    struct MockInitServices;

    impl InitServices for MockInitServices {
        async fn generate_keypair(&mut self) -> anyhow::Result<DytallixKeypair> {
            Ok(DytallixKeypair::generate())
        }

        async fn save_keypair(&mut self, _keypair: &DytallixKeypair) -> anyhow::Result<PathBuf> {
            Ok(PathBuf::from("/tmp/keystore.json"))
        }

        async fn submit_faucet(&mut self, _address: &DAddr) -> anyhow::Result<()> {
            Ok(())
        }

        async fn poll_balance(&mut self, _address: &DAddr) -> anyhow::Result<Balance> {
            Ok(Balance {
                dgt: 1_000,
                drt: 10_000,
            })
        }

        async fn wait(&mut self, _duration: Duration) {}
    }

    #[tokio::test]
    async fn elapsed_time_is_real() {
        let mut services = MockInitServices;
        let mut lines = Vec::new();
        let elapsed = run_with_services(&mut services, |line| lines.push(line))
            .await
            .unwrap();
        assert!(elapsed > Duration::ZERO);
    }

    struct DrtOnlyInitServices;

    impl InitServices for DrtOnlyInitServices {
        async fn generate_keypair(&mut self) -> anyhow::Result<DytallixKeypair> {
            Ok(DytallixKeypair::generate())
        }

        async fn save_keypair(&mut self, _keypair: &DytallixKeypair) -> anyhow::Result<PathBuf> {
            Ok(PathBuf::from("/tmp/keystore.json"))
        }

        async fn submit_faucet(&mut self, _address: &DAddr) -> anyhow::Result<()> {
            Ok(())
        }

        async fn poll_balance(&mut self, _address: &DAddr) -> anyhow::Result<Balance> {
            // The faucet dispenses DRT only; DGT is never funded this way.
            Ok(Balance {
                dgt: 0,
                drt: 10_000,
            })
        }

        async fn wait(&mut self, _duration: Duration) {}
    }

    #[tokio::test]
    async fn funding_confirms_on_drt_when_dgt_stays_zero() {
        // Regression: init previously required `dgt > 0 && drt > 0`, which could
        // never confirm because the faucet does not dispense DGT — so a real
        // freshly-funded wallet always hit the 45s timeout. Confirming on DRT
        // alone must succeed and must not claim a DGT credit that never happened.
        let mut services = DrtOnlyInitServices;
        let mut lines = Vec::new();
        let result = run_with_services(&mut services, |line| lines.push(line)).await;
        assert!(result.is_ok(), "init should confirm funding on DRT alone");
        assert!(
            lines.iter().any(|line| line.contains("DRT received")),
            "should report DRT funding"
        );
        assert!(
            !lines.iter().any(|line| line.contains("DGT received")),
            "should not report DGT when the faucet dispensed none"
        );
    }
}
