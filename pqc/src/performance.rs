//! Performance benchmarking for PQC signatures in bridge operations
//!
//! Measures gas costs, compute overhead, and provides optimization recommendations.

use crate::{BridgePQCManager, CrossChainPayload, SignatureAlgorithm};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Type alias to simplify complex return type used in benchmarking
pub type BenchOperationResult = (Duration, Duration, Duration, usize, usize);

/// Benchmark results for different PQC algorithms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PQCBenchmarkResults {
    pub algorithm: String,
    pub key_generation_time: Duration,
    pub signature_time: Duration,
    pub verification_time: Duration,
    pub signature_size: usize,
    pub public_key_size: usize,
    pub estimated_gas_cost: u64,
    pub operations_per_second: f64,
}

/// Gas cost estimation for different blockchain networks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasCostEstimation {
    pub network: String,
    pub base_transaction_cost: u64,
    pub signature_verification_cost: u64,
    pub storage_cost_per_byte: u64,
    pub total_estimated_cost: u64,
    pub cost_in_usd: f64,
}

/// Performance analysis and optimization recommendations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceAnalysis {
    pub benchmarks: Vec<PQCBenchmarkResults>,
    pub gas_estimations: Vec<GasCostEstimation>,
    pub recommendations: Vec<String>,
    pub optimal_algorithm: String,
}

pub struct PQCPerformanceBenchmark {
    pqc_manager: BridgePQCManager,
    results: Vec<PQCBenchmarkResults>,
}

impl PQCPerformanceBenchmark {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let pqc_manager = BridgePQCManager::new()?;

        Ok(Self {
            pqc_manager,
            results: Vec::new(),
        })
    }

    /// Run comprehensive benchmarks for all supported PQC algorithms
    pub fn run_comprehensive_benchmarks(
        &mut self,
    ) -> Result<PerformanceAnalysis, Box<dyn std::error::Error>> {
        #[cfg(feature = "benchmark-stdout")]
        println!("🚀 Starting comprehensive PQC performance benchmarks...\n");

        let algorithms = vec![
            ("Dilithium5", SignatureAlgorithm::Dilithium5),
            ("Falcon1024", SignatureAlgorithm::Falcon1024),
            ("SPHINCS+", SignatureAlgorithm::SphincsSha256128s),
        ];

        self.results.clear();

        for (name, algorithm) in algorithms {
            #[cfg(feature = "benchmark-stdout")]
            println!("📊 Benchmarking {name}...");
            let result = self.benchmark_algorithm(name, &algorithm)?;
            self.results.push(result);
            #[cfg(feature = "benchmark-stdout")]
            println!("✅ {name} benchmark completed\n");
        }

        let gas_estimations = self.estimate_gas_costs(&self.results)?;
        let recommendations = self.generate_recommendations(&self.results, &gas_estimations);
        let optimal_algorithm = self.determine_optimal_algorithm(&self.results);

        Ok(PerformanceAnalysis {
            benchmarks: self.results.clone(),
            gas_estimations,
            recommendations,
            optimal_algorithm,
        })
    }

    /// Benchmark a specific PQC algorithm
    fn benchmark_algorithm(
        &mut self,
        name: &str,
        algorithm: &SignatureAlgorithm,
    ) -> Result<PQCBenchmarkResults, Box<dyn std::error::Error>> {
        const ITERATIONS: u32 = 10;
        const WARMUP_ITERATIONS: u32 = 3;

        // Warmup
        for _ in 0..WARMUP_ITERATIONS {
            self.single_operation_benchmark(algorithm)?;
        }

        // Actual benchmarking
        let mut key_gen_times = Vec::new();
        let mut signature_times = Vec::new();
        let mut verification_times = Vec::new();
        let mut signature_size = 0;
        let mut public_key_size = 0;

        for i in 0..ITERATIONS {
            let (key_gen_time, sig_time, verif_time, sig_size, pub_key_size) =
                self.single_operation_benchmark(algorithm)?;

            key_gen_times.push(key_gen_time);
            signature_times.push(sig_time);
            verification_times.push(verif_time);

            if i == 0 {
                signature_size = sig_size;
                public_key_size = pub_key_size;
            }

            if i % 3 == 0 {
                print!(".");
                std::io::Write::flush(&mut std::io::stdout()).unwrap();
            }
        }
        println!();

        // Calculate averages
        let avg_key_gen = key_gen_times.iter().sum::<Duration>() / ITERATIONS;
        let avg_signature = signature_times.iter().sum::<Duration>() / ITERATIONS;
        let avg_verification = verification_times.iter().sum::<Duration>() / ITERATIONS;

        // Calculate operations per second (for signature + verification)
        let total_op_time = avg_signature + avg_verification;
        let ops_per_second = if total_op_time.as_secs_f64() > 0.0 {
            1.0 / total_op_time.as_secs_f64()
        } else {
            0.0
        };

        // Estimate gas cost
        let estimated_gas = self.estimate_algorithm_gas_cost(algorithm, signature_size)?;

        Ok(PQCBenchmarkResults {
            algorithm: name.to_string(),
            key_generation_time: avg_key_gen,
            signature_time: avg_signature,
            verification_time: avg_verification,
            signature_size,
            public_key_size,
            estimated_gas_cost: estimated_gas,
            operations_per_second: ops_per_second,
        })
    }

    /// Perform a single operation benchmark (key gen, sign, verify)
    fn single_operation_benchmark(
        &mut self,
        algorithm: &SignatureAlgorithm,
    ) -> Result<BenchOperationResult, Box<dyn std::error::Error>> {
        // Key generation benchmark
        let key_gen_start = Instant::now();
        let keypair = self.pqc_manager.generate_validator_keypair(algorithm)?;
        let key_gen_time = key_gen_start.elapsed();

        // Add validator for signing
        let validator_id = format!(
            "bench_validator_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        self.pqc_manager.add_validator(
            validator_id.clone(),
            keypair.public_key.clone(),
            algorithm.clone(),
        );

        // Create test payload
        let payload = CrossChainPayload::GenericBridgePayload {
            asset_id: "BENCHMARK_TOKEN".to_string(),
            amount: 1000000,
            source_chain: "ethereum".to_string(),
            dest_chain: "cosmos".to_string(),
            source_address: "0x742d35Cc6634C0532925a3b8D1EbA4F00b7C8000".to_string(),
            dest_address: "cosmos1benchmark".to_string(),
            metadata: HashMap::new(),
        };

        // Signature benchmark
        let signature_start = Instant::now();
        let signature =
            self.pqc_manager
                .sign_bridge_payload(&payload, "ethereum", &validator_id)?;
        let signature_time = signature_start.elapsed();

        // Verification benchmark
        let verification_start = Instant::now();
        let _is_valid = self
            .pqc_manager
            .verify_bridge_signature(&signature, &payload)?;
        let verification_time = verification_start.elapsed();

        Ok((
            key_gen_time,
            signature_time,
            verification_time,
            signature.signature.data.len(),
            keypair.public_key.len(),
        ))
    }

    /// Estimate gas costs for different algorithms
    fn estimate_algorithm_gas_cost(
        &self,
        algorithm: &SignatureAlgorithm,
        signature_size: usize,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        // Gas cost estimation based on algorithm complexity and signature size
        let base_verification_cost = match algorithm {
            SignatureAlgorithm::Dilithium3 => 1000, // Lower cost than Dilithium5
            SignatureAlgorithm::Dilithium5 => 1500, // Moderate cost
            SignatureAlgorithm::Falcon1024 => 1200, // Lower cost, more efficient
            SignatureAlgorithm::SphincsSha256128s => 2500, // Higher cost due to complexity
        };

        // Additional cost for signature size (storage and processing)
        let size_cost = (signature_size as u64) / 32 * 20; // ~20 gas per 32-byte word

        // Multi-signature overhead (assuming 3-of-5 multi-sig)
        let multisig_overhead = base_verification_cost * 3;

        Ok(base_verification_cost + size_cost + multisig_overhead)
    }

    /// Estimate gas costs for different blockchain networks
    fn estimate_gas_costs(
        &self,
        results: &[PQCBenchmarkResults],
    ) -> Result<Vec<GasCostEstimation>, Box<dyn std::error::Error>> {
        let networks = vec![
            ("Ethereum", 21000, 20_000_000_000u64, 1800.0), // (base_cost, gas_price_wei, eth_price_usd)
            ("Polygon", 21000, 30_000_000_000u64, 0.8), // (base_cost, gas_price_wei, matic_price_usd)
            ("Cosmos", 200000, 25u64, 10.0), // (base_cost, gas_price_uatom, atom_price_usd) - simplified
        ];

        let mut estimations = Vec::new();

        for result in results {
            for (network_name, base_cost, gas_price_raw, token_price_usd) in &networks {
                let total_gas = base_cost + result.estimated_gas_cost;

                let cost_in_usd = if network_name == &"Cosmos" {
                    // Cosmos uses different gas calculation
                    (total_gas as f64 * (*gas_price_raw as f64) * token_price_usd) / 1_000_000.0
                } else {
                    // Ethereum-like networks
                    let cost_wei = total_gas * gas_price_raw;
                    let cost_token = cost_wei as f64 / 1e18;
                    cost_token * token_price_usd
                };

                estimations.push(GasCostEstimation {
                    network: format!("{} ({})", network_name, result.algorithm),
                    base_transaction_cost: *base_cost,
                    signature_verification_cost: result.estimated_gas_cost,
                    storage_cost_per_byte: 20, // Simplified
                    total_estimated_cost: total_gas,
                    cost_in_usd,
                });
            }
        }

        Ok(estimations)
    }

    /// Generate optimization recommendations based on benchmark results
    fn generate_recommendations(
        &self,
        results: &[PQCBenchmarkResults],
        gas_estimations: &[GasCostEstimation],
    ) -> Vec<String> {
        let mut recommendations = Vec::new();

        // Find fastest algorithm
        let fastest_signature = results.iter().min_by_key(|r| r.signature_time);
        let fastest_verification = results.iter().min_by_key(|r| r.verification_time);
        let smallest_signature = results.iter().min_by_key(|r| r.signature_size);
        let lowest_gas = results.iter().min_by_key(|r| r.estimated_gas_cost);

        if let Some(fastest_sig) = fastest_signature {
            recommendations.push(format!(
                "Fastest signature generation: {} ({:.2}ms)",
                fastest_sig.algorithm,
                fastest_sig.signature_time.as_millis()
            ));
        }

        if let Some(fastest_verif) = fastest_verification {
            recommendations.push(format!(
                "Fastest verification: {} ({:.2}ms)",
                fastest_verif.algorithm,
                fastest_verif.verification_time.as_millis()
            ));
        }

        if let Some(smallest_sig) = smallest_signature {
            recommendations.push(format!(
                "Smallest signature size: {} ({} bytes)",
                smallest_sig.algorithm, smallest_sig.signature_size
            ));
        }

        if let Some(lowest_gas_cost) = lowest_gas {
            recommendations.push(format!(
                "Lowest estimated gas cost: {} ({} gas units)",
                lowest_gas_cost.algorithm, lowest_gas_cost.estimated_gas_cost
            ));
        }

        // Find lowest cost network
        let lowest_cost_network = gas_estimations
            .iter()
            .min_by(|a, b| a.cost_in_usd.partial_cmp(&b.cost_in_usd).unwrap());
        if let Some(lowest_cost) = lowest_cost_network {
            recommendations.push(format!(
                "Most cost-effective network: {} (${:.4} USD)",
                lowest_cost.network, lowest_cost.cost_in_usd
            ));
        }

        // General optimization recommendations
        recommendations
            .push("Consider batching multiple signatures for better gas efficiency".to_string());
        recommendations.push(
            "Implement signature aggregation where possible to reduce verification overhead"
                .to_string(),
        );
        recommendations.push(
            "Use Falcon1024 for latency-critical applications due to faster verification"
                .to_string(),
        );
        recommendations.push("Use Dilithium5 for balanced security and performance".to_string());
        recommendations
            .push("Consider SPHINCS+ only for maximum long-term security requirements".to_string());

        recommendations
    }

    /// Determine the optimal algorithm based on overall performance
    fn determine_optimal_algorithm(&self, results: &[PQCBenchmarkResults]) -> String {
        // Score each algorithm based on multiple factors
        let mut scores: HashMap<String, f64> = HashMap::new();

        for result in results {
            let mut score = 0.0;

            // Faster operations get higher scores
            score += 1000.0 / (result.signature_time.as_millis() as f64 + 1.0);
            score += 1000.0 / (result.verification_time.as_millis() as f64 + 1.0);

            // Lower gas costs get higher scores
            score += 10000.0 / (result.estimated_gas_cost as f64 + 1.0);

            // Smaller signature sizes get higher scores
            score += 100000.0 / (result.signature_size as f64 + 1.0);

            // Higher operations per second get higher scores
            score += result.operations_per_second * 1000.0;

            scores.insert(result.algorithm.clone(), score);
        }

        scores
            .into_iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(algorithm, _)| algorithm)
            .unwrap_or_else(|| "Dilithium5".to_string())
    }

    /// Print detailed performance report
    pub fn print_performance_report(&self, analysis: &PerformanceAnalysis) {
        println!("\n📊 COMPREHENSIVE PQC PERFORMANCE ANALYSIS REPORT");
        println!("{}", "=".repeat(60));

        // Algorithm comparison table
        println!("\n🔍 Algorithm Performance Comparison:");
        println!("{:-<100}", "");
        println!(
            "{:<12} {:<15} {:<15} {:<15} {:<12} {:<12} {:<8}",
            "Algorithm",
            "Key Gen (ms)",
            "Sign (ms)",
            "Verify (ms)",
            "Sig Size",
            "Gas Cost",
            "Ops/sec"
        );
        println!("{:-<100}", "");

        for result in &analysis.benchmarks {
            println!(
                "{:<12} {:<15.2} {:<15.2} {:<15.2} {:<12} {:<12} {:<8.1}",
                result.algorithm,
                result.key_generation_time.as_millis(),
                result.signature_time.as_millis(),
                result.verification_time.as_millis(),
                result.signature_size,
                result.estimated_gas_cost,
                result.operations_per_second
            );
        }

        // Gas cost analysis
        println!("\n⛽ Gas Cost Analysis:");
        println!("{:-<80}", "");
        println!(
            "{:<30} {:<15} {:<15} {:<15}",
            "Network (Algorithm)", "Total Gas", "Est. Cost USD", "Efficiency"
        );
        println!("{:-<80}", "");

        for estimation in &analysis.gas_estimations {
            let efficiency = if estimation.cost_in_usd > 0.0 {
                format!("{:.1}x", 1.0 / estimation.cost_in_usd)
            } else {
                "N/A".to_string()
            };

            println!(
                "{:<30} {:<15} ${:<14.4} {:<15}",
                estimation.network,
                estimation.total_estimated_cost,
                estimation.cost_in_usd,
                efficiency
            );
        }

        // Recommendations
        println!("\n💡 Optimization Recommendations:");
        println!("{:-<50}", "");
        for (i, recommendation) in analysis.recommendations.iter().enumerate() {
            println!("{}. {}", i + 1, recommendation);
        }

        // Optimal algorithm
        println!("\n🏆 Recommended Algorithm: {}", analysis.optimal_algorithm);

        // Summary
        println!("\n📈 Summary:");
        println!(
            "  • Total algorithms benchmarked: {}",
            analysis.benchmarks.len()
        );
        println!("  • Best overall algorithm: {}", analysis.optimal_algorithm);

        if let Some(best) = analysis
            .benchmarks
            .iter()
            .find(|r| r.algorithm == analysis.optimal_algorithm)
        {
            println!(
                "  • Best algorithm signature time: {:.2}ms",
                best.signature_time.as_millis()
            );
            println!(
                "  • Best algorithm verification time: {:.2}ms",
                best.verification_time.as_millis()
            );
            println!(
                "  • Best algorithm gas cost: {} units",
                best.estimated_gas_cost
            );
        }

        println!("\n{}", "=".repeat(60));
    }

    /// Export results to JSON for further analysis
    pub fn export_results(
        &self,
        analysis: &PerformanceAnalysis,
        filename: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let json_data = serde_json::to_string_pretty(analysis)?;
        std::fs::write(filename, json_data)?;
        println!("📄 Results exported to: {filename}");
        Ok(())
    }
}

/// Run performance benchmarks and generate report
pub fn run_pqc_performance_benchmarks() -> Result<(), Box<dyn std::error::Error>> {
    let mut benchmark = PQCPerformanceBenchmark::new()?;
    let analysis = benchmark.run_comprehensive_benchmarks()?;

    benchmark.print_performance_report(&analysis);

    // Export results
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let filename = format!("pqc_benchmark_results_{timestamp}.json");
    benchmark.export_results(&analysis, &filename)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_benchmark() {
        let mut benchmark = PQCPerformanceBenchmark::new().unwrap();
        let analysis = benchmark.run_comprehensive_benchmarks().unwrap();

        // Verify we have results for all algorithms
        assert_eq!(analysis.benchmarks.len(), 3);

        // Verify gas estimations were generated
        assert!(!analysis.gas_estimations.is_empty());

        // Verify recommendations were generated
        assert!(!analysis.recommendations.is_empty());

        // Verify optimal algorithm was determined
        assert!(!analysis.optimal_algorithm.is_empty());

        println!("✅ Performance benchmark test completed");
    }

    #[test]
    fn test_gas_cost_estimation() {
        let benchmark = PQCPerformanceBenchmark::new().unwrap();

        // Test gas cost estimation for different algorithms
        let dilithium_gas = benchmark
            .estimate_algorithm_gas_cost(&SignatureAlgorithm::Dilithium5, 2420)
            .unwrap();
        let falcon_gas = benchmark
            .estimate_algorithm_gas_cost(&SignatureAlgorithm::Falcon1024, 690)
            .unwrap();
        let sphincs_gas = benchmark
            .estimate_algorithm_gas_cost(&SignatureAlgorithm::SphincsSha256128s, 7856)
            .unwrap();

        // SPHINCS+ should have highest gas cost due to large signatures
        assert!(sphincs_gas > dilithium_gas);
        assert!(sphincs_gas > falcon_gas);

        // All should be reasonable values
        assert!(dilithium_gas > 0);
        assert!(falcon_gas > 0);
        assert!(sphincs_gas > 0);

        println!("✅ Gas cost estimation test completed");
        println!("  - Dilithium5: {dilithium_gas} gas");
        println!("  - Falcon1024: {falcon_gas} gas");
        println!("  - SPHINCS+: {sphincs_gas} gas");
    }
}
