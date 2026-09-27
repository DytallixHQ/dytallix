//! Qualification fixture only. Runs the supervisor's startup profile check
//! (`require_enforced_profiles`) against the live kernel for four effective
//! role labels, so native runs can confirm that absent or complain-mode
//! profiles are refused.
fn main() {
    let labels: Vec<String> = std::env::args().skip(1).collect();
    if labels.len() != 4 {
        eprintln!("Usage: SUPERVISOR APPLICATION_OWNER WORKLOAD HELPER (effective labels)");
        std::process::exit(2);
    }
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    match dytallix_release_runtime::ownership_security::require_enforced_profiles(&labels) {
        Ok(()) => println!("ADMITTED"),
        Err(error) => {
            println!("REFUSED {error:#}");
            std::process::exit(1);
        }
    }
}
