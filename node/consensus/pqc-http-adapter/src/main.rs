use dytallix_release_runtime::ownership::{self, Role};

// The entry point retains the process-leader owner guard before it creates the
// current-thread runtime or opens an HTTP listener.
fn run() -> Result<(), String> {
    ownership::install_cancellation().map_err(|e| e.to_string())?;
    let admission = ownership::admit(Role::Adapter).map_err(|e| e.to_string())?;
    let (context, args) = release_context_args(std::env::args().skip(1))?;
    admission
        .verify_context(&context)
        .map_err(|e| e.to_string())?;
    let config = dytallix_pqc_http_adapter::Config::parse(args)?;
    admission.check().map_err(|e| e.to_string())?;
    ownership::check_cancellation().map_err(|e| e.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    // Admission stays on this thread until the runtime and listener stop.
    runtime.block_on(async {
        let service = dytallix_pqc_http_adapter::serve(config);
        tokio::pin!(service);
        loop {
            tokio::select! {
                result = &mut service => return result,
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    admission.check().map_err(|e| e.to_string())?;
                    ownership::check_cancellation().map_err(|e| e.to_string())?;
                }
            }
        }
    })
}

fn release_context_args(
    args: impl IntoIterator<Item = String>,
) -> Result<([u8; 64], Vec<String>), String> {
    let mut args = args.into_iter();
    let mut context = None;
    let mut remaining = Vec::new();
    while let Some(arg) = args.next() {
        if arg == "--release-manifest-sha512" {
            if context.is_some() {
                return Err("Duplicate release manifest digest".into());
            }
            let value = args.next().ok_or("Missing release manifest digest")?;
            context = Some(ownership::parse_context_sha512(&value).map_err(|e| e.to_string())?);
        } else {
            remaining.push(arg);
        }
    }
    Ok((
        context.ok_or("Explicit release manifest digest required")?,
        remaining,
    ))
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }
    #[test]
    fn context_is_mandatory_exact_and_unique() {
        for value in [
            "",
            "00",
            &"AA".repeat(64),
            &"gg".repeat(64),
            &"00".repeat(64),
        ] {
            assert!(release_context_args(args(&["--release-manifest-sha512", value])).is_err());
        }
        assert!(release_context_args(args(&["--profile", "test"])).is_err());
        assert!(release_context_args(args(&["--release-manifest-sha512"])).is_err());
        let digest = "ab".repeat(64);
        assert!(release_context_args(args(&[
            "--release-manifest-sha512",
            &digest,
            "--release-manifest-sha512",
            &digest
        ]))
        .is_err());
    }
    #[test]
    fn context_extraction_preserves_existing_argument_validation() {
        let digest = "ab".repeat(64);
        let (context, remaining) = release_context_args(args(&[
            "--profile",
            "test",
            "--release-manifest-sha512",
            &digest,
            "--production",
        ]))
        .unwrap();
        assert_eq!(context, [0xab; 64]);
        assert_eq!(remaining, args(&["--profile", "test", "--production"]));
    }
}
