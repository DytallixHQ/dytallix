//! The attack campaign's test-only client (launch/approvals/
//! P01_T04_ATTACK_DECISIONS_2026-11-04.json): it sends what the release's
//! own client never would. It is built and run only by the campaign and CI,
//! never shipped in a release or a host bundle.
//!
//! `dytallix-attack ordinary-forge` re-signs a valid ordinary transaction
//! (the release CLI's transport) after one change the CLI refuses to sign,
//! with the account's own key, as an attacker holding it can:
//!
//! - `fee_cap_below_minimum`: the fee cap set `k + 1` below the gas limit
//!   times the profile's gas price (the minimum), the gas unchanged;
//! - `too_many_actions`: the first action repeated to one more than the
//!   profile's action limit, encoded by protocol-types under that limit
//!   raised by one, so the bytes are canonical and only the count is wrong.
//!
//! The signature is pure ML-DSA-65 with an empty context over the whole
//! signing bytes (the SDK's `OrdinarySigner`), checked before it is written.
//! The forgery is checked too: the profile must refuse it for the variant's
//! reason, and the variant's change must be the only one. It writes the
//! transport and prints the checks as one JSON line.
use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use dytallix_protocol_types::ordinary::{self, Limits, SignedOrdinary};
use dytallix_protocol_types::ordinary_fees::FeeProfile;
use fips204::ml_dsa_65;
use fips204::traits::{SerDes, Signer, Verifier};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const ALGORITHM: &str = "mldsa65";
const VARIANTS: [&str; 2] = ["fee_cap_below_minimum", "too_many_actions"];
/// What the profile says of each forgery (protocol-types).
const FEE_CAP_REFUSAL: &str = "signed fee cap outside ordinary profile";
const ACTION_COUNT_REFUSAL: &str = "action count outside profile";

#[derive(Debug, PartialEq)]
struct Forge {
    variant: String,
    transport: PathBuf,
    key_file: PathBuf,
    profile: PathBuf,
    k: u64,
    out: PathBuf,
}

fn parse(args: &[String]) -> Result<Forge> {
    ensure!(
        args.first().map(String::as_str) == Some("ordinary-forge"),
        "usage: dytallix-attack ordinary-forge --variant V --transport FILE --key-file FILE --profile FILE --k N --out FILE"
    );
    let mut found = std::collections::BTreeMap::new();
    let mut rest = args[1..].iter();
    while let Some(flag) = rest.next() {
        let value = rest
            .next()
            .with_context(|| format!("{flag} needs a value"))?;
        ensure!(
            [
                "--variant",
                "--transport",
                "--key-file",
                "--profile",
                "--k",
                "--out"
            ]
            .contains(&flag.as_str()),
            "unknown argument {flag}"
        );
        ensure!(
            found.insert(flag.as_str(), value.clone()).is_none(),
            "{flag} given twice"
        );
    }
    let take = |flag: &str| {
        found
            .get(flag)
            .cloned()
            .with_context(|| format!("{flag} is required"))
    };
    let variant = take("--variant")?;
    ensure!(
        VARIANTS.contains(&variant.as_str()),
        "{variant} is not a forged variant"
    );
    Ok(Forge {
        variant,
        transport: take("--transport")?.into(),
        key_file: take("--key-file")?.into(),
        profile: take("--profile")?.into(),
        k: take("--k")?.parse().context("--k is a whole number")?,
        out: take("--out")?.into(),
    })
}

fn read_json(path: &Path) -> Result<Value> {
    serde_json::from_slice(
        &std::fs::read(path).with_context(|| format!("reading {}", path.display()))?,
    )
    .with_context(|| format!("{} is not JSON", path.display()))
}

/// The committed fee profile in an ordinary profile view (`config.fee_profile`).
fn fee_profile(view: &Value) -> Result<FeeProfile> {
    let fee = view
        .pointer("/config/fee_profile")
        .context("the profile view has no config.fee_profile")?;
    let profile: FeeProfile =
        serde_json::from_value(fee.clone()).context("the fee profile does not parse")?;
    profile
        .validate()
        .map_err(|e| anyhow!("the fee profile is invalid: {e}"))?;
    Ok(profile)
}

fn envelope(transport: &Value) -> Result<Vec<u8>> {
    ensure!(
        transport["type"] == "ordinary_v2",
        "the transport is not ordinary_v2"
    );
    let text = transport["envelope_base64"]
        .as_str()
        .context("the transport has no envelope")?;
    STANDARD.decode(text).context("the envelope is not base64")
}

struct Key {
    public: Vec<u8>,
    private: ml_dsa_65::PrivateKey,
}

/// A load wallet's key file (load.py `make_wallet`): the CLI's own export.
fn key(file: &Value) -> Result<Key> {
    ensure!(
        file["algorithm"] == ALGORITHM,
        "the key file is not {ALGORITHM}"
    );
    let bytes = |name: &str| -> Result<Vec<u8>> {
        file[name]
            .as_array()
            .with_context(|| format!("the key file has no {name}"))?
            .iter()
            .map(|b| {
                b.as_u64()
                    .and_then(|b| u8::try_from(b).ok())
                    .context("a key byte is not a byte")
            })
            .collect()
    };
    let public = bytes("public_key")?;
    let private: [u8; ml_dsa_65::SK_LEN] = bytes("private_key")?
        .try_into()
        .map_err(|_| anyhow!("the private key is not ML-DSA-65"))?;
    let private = ml_dsa_65::PrivateKey::try_from_bytes(private)
        .map_err(|e| anyhow!("the private key: {e}"))?;
    Ok(Key { public, private })
}

fn sign(key: &Key, message: &[u8]) -> Result<Vec<u8>> {
    let signature = key
        .private
        .try_sign(message, &[])
        .map_err(|e| anyhow!("signing failed: {e}"))?;
    let public: [u8; ml_dsa_65::PK_LEN] = key
        .public
        .clone()
        .try_into()
        .map_err(|_| anyhow!("the public key is not ML-DSA-65"))?;
    let public =
        ml_dsa_65::PublicKey::try_from_bytes(public).map_err(|e| anyhow!("the public key: {e}"))?;
    ensure!(
        public.verify(message, &signature, &[]),
        "the signature does not verify with the account's key"
    );
    Ok(signature.to_vec())
}

/// The forged envelope and its checks.
fn forge(
    original: &[u8],
    key: &Key,
    profile: &FeeProfile,
    variant: &str,
    k: u64,
) -> Result<(Vec<u8>, Value)> {
    let limits = &profile.limits;
    let signed = ordinary::decode(original, limits)
        .map_err(|e| anyhow!("the original does not decode: {e}"))?;
    ensure!(
        signed.body.key.algorithm == ALGORITHM && signed.body.key.public_key == key.public,
        "the key file is not the transaction's account key"
    );
    let mut body = signed.body.clone();
    let mut encoding: Limits = limits.clone();
    let minimum = u128::from(body.gas_limit)
        .checked_mul(u128::from(profile.gas_price))
        .context("fee bound overflow")?;
    match variant {
        "fee_cap_below_minimum" => {
            body.maximum_fee = minimum
                .checked_sub(1 + u128::from(k))
                .context("k is past the minimum")?;
        }
        "too_many_actions" => {
            let first = body
                .actions
                .first()
                .cloned()
                .context("the original has no action")?;
            let count = usize::from(limits.max_actions) + 1;
            body.actions = vec![first; count];
            encoding.max_actions = limits
                .max_actions
                .checked_add(1)
                .context("the action limit is at its maximum")?;
        }
        other => bail!("{other} is not a forged variant"),
    }
    let message =
        ordinary::signing_bytes(&body, &encoding).map_err(|e| anyhow!("encoding: {e}"))?;
    let forged = SignedOrdinary {
        body: body.clone(),
        signature: sign(key, &message)?,
    };
    let bytes = ordinary::encode(&forged, &encoding).map_err(|e| anyhow!("encoding: {e}"))?;
    // The forgery round-trips under the encoding limits, and the change is
    // the only one: everything else is the original's.
    let again = ordinary::decode(&bytes, &encoding)
        .map_err(|e| anyhow!("the forgery does not decode: {e}"))?;
    ensure!(again == forged, "the forgery does not round-trip");
    let mut unchanged = signed.body.clone();
    unchanged.maximum_fee = body.maximum_fee;
    unchanged.actions = body.actions.clone();
    ensure!(
        unchanged == body,
        "the forgery changed more than its variant"
    );
    // What the profile itself says of it.
    let refusal = match variant {
        "fee_cap_below_minimum" => profile
            .validate_request(body.gas_limit, body.maximum_fee)
            .err(),
        _ => ordinary::decode(&bytes, limits).err(),
    }
    .map(|e| e.to_string());
    let wanted = if variant == "fee_cap_below_minimum" {
        FEE_CAP_REFUSAL
    } else {
        ACTION_COUNT_REFUSAL
    };
    ensure!(
        refusal.as_deref().is_some_and(|r| r.contains(wanted)),
        "the profile does not refuse the forgery for its reason: {refusal:?}"
    );
    let report = json!({
        "variant": variant,
        "k": k,
        "transaction_id": hex(&ordinary::transaction_id(&body, &encoding).map_err(|e| anyhow!("{e}"))?),
        "minimum_fee": minimum.to_string(),
        "maximum_fee": body.maximum_fee.to_string(),
        "actions": body.actions.len(),
        "max_actions": limits.max_actions,
        "signature_verifies": true,
        "profile_refusal": wanted,
    });
    Ok((bytes, report))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn run(args: &[String]) -> Result<Value> {
    let forge_args = parse(args)?;
    let profile = fee_profile(&read_json(&forge_args.profile)?)?;
    let original = envelope(&read_json(&forge_args.transport)?)?;
    let key = key(&read_json(&forge_args.key_file)?)?;
    let (bytes, report) = forge(&original, &key, &profile, &forge_args.variant, forge_args.k)?;
    let transport = json!({"type": "ordinary_v2", "envelope_base64": STANDARD.encode(&bytes)});
    std::fs::write(&forge_args.out, serde_json::to_vec(&transport)?)
        .with_context(|| format!("writing {}", forge_args.out.display()))?;
    Ok(report)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(report) => println!("{report}"),
        Err(error) => {
            eprintln!("dytallix-attack: {error:#}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dytallix_protocol_types::ordinary::{Action, Denomination, OrdinaryTransaction};
    use dytallix_protocol_types::recovery::{KeyIdentity, RecoveryDomain};
    use fips204::traits::KeyGen;

    /// The rehearsal genesis's fee profile, as a profile view carries it.
    fn profile() -> FeeProfile {
        let config = include_str!(
            "../../mainnet-preparation/fixtures/genesis-rehearsal/application-config.json"
        );
        let config: Value = serde_json::from_str(config).unwrap();
        fee_profile(&json!({"config": {"fee_profile": config["ordinary"]["fee_profile"]}})).unwrap()
    }

    fn wallet() -> Key {
        let (public, private) = ml_dsa_65::KG::keygen_from_seed(&[7; 32]);
        Key {
            public: public.into_bytes().to_vec(),
            private,
        }
    }

    /// A valid send the release CLI could have signed.
    fn original(key: &Key, profile: &FeeProfile) -> Vec<u8> {
        let body = OrdinaryTransaction {
            domain: RecoveryDomain {
                network: 3,
                chain_id: "dytallix-staging-1".into(),
                genesis_digest: [1; 32],
                account_id: [2; 32],
            },
            authorization_generation: 0,
            spending_nonce: 4,
            key: KeyIdentity {
                algorithm: ALGORITHM.into(),
                public_key: key.public.clone(),
            },
            expiry_height: 90,
            ordinary_fee_contract_version: profile.ordinary_fee_contract_version,
            fee_profile_version: profile.version,
            fee_profile_digest: [3; 32],
            fee_denomination: Denomination::Udrt,
            maximum_fee: 200_000 * u128::from(profile.gas_price),
            gas_limit: 200_000,
            memo: "t4".into(),
            actions: vec![Action::Send {
                recipient: [5; 32],
                denomination: Denomination::Udrt,
                amount: 1,
            }],
        };
        profile
            .validate_request(body.gas_limit, body.maximum_fee)
            .unwrap();
        let message = ordinary::signing_bytes(&body, &profile.limits).unwrap();
        let signed = SignedOrdinary {
            signature: sign(key, &message).unwrap(),
            body,
        };
        ordinary::encode(&signed, &profile.limits).unwrap()
    }

    fn verifies(bytes: &[u8], limits: &Limits, key: &Key) -> bool {
        let signed = ordinary::decode(bytes, limits).unwrap();
        let public: [u8; ml_dsa_65::PK_LEN] = key.public.clone().try_into().unwrap();
        let message = ordinary::signing_bytes(&signed.body, limits).unwrap();
        let signature: [u8; ml_dsa_65::SIG_LEN] = signed.signature.try_into().unwrap();
        ml_dsa_65::PublicKey::try_from_bytes(public)
            .unwrap()
            .verify(&message, &signature, &[])
    }

    #[test]
    fn a_fee_cap_below_the_minimum_is_validly_signed_and_refused_by_the_profile() {
        let (profile, key) = (profile(), wallet());
        let original = original(&key, &profile);
        for k in [0, 41] {
            let (bytes, report) =
                forge(&original, &key, &profile, "fee_cap_below_minimum", k).unwrap();
            let signed = ordinary::decode(&bytes, &profile.limits).unwrap();
            let minimum = 200_000 * u128::from(profile.gas_price);
            assert_eq!(signed.body.maximum_fee, minimum - 1 - u128::from(k));
            assert!(verifies(&bytes, &profile.limits, &key));
            assert_eq!(report["profile_refusal"], FEE_CAP_REFUSAL);
            assert_eq!(
                report["maximum_fee"],
                (minimum - 1 - u128::from(k)).to_string()
            );
        }
    }

    #[test]
    fn one_action_too_many_is_canonical_validly_signed_and_refused_only_for_its_count() {
        let (profile, key) = (profile(), wallet());
        let (bytes, report) = forge(
            &original(&key, &profile),
            &key,
            &profile,
            "too_many_actions",
            0,
        )
        .unwrap();
        let error = ordinary::decode(&bytes, &profile.limits)
            .unwrap_err()
            .to_string();
        assert!(error.contains(ACTION_COUNT_REFUSAL), "{error}");
        let mut raised = profile.limits.clone();
        raised.max_actions += 1;
        assert_eq!(
            ordinary::decode(&bytes, &raised)
                .unwrap()
                .body
                .actions
                .len(),
            17
        );
        assert!(verifies(&bytes, &raised, &key));
        assert_eq!(
            (report["actions"].as_u64(), report["max_actions"].as_u64()),
            (Some(17), Some(16))
        );
    }

    #[test]
    fn another_key_or_a_broken_original_is_refused() {
        let (profile, key) = (profile(), wallet());
        let original = original(&key, &profile);
        let (public, private) = ml_dsa_65::KG::keygen_from_seed(&[9; 32]);
        let other = Key {
            public: public.into_bytes().to_vec(),
            private,
        };
        let error = forge(&original, &other, &profile, "too_many_actions", 0)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("not the transaction's account key"),
            "{error}"
        );
        let mut broken = original.clone();
        broken.push(0);
        assert!(forge(&broken, &key, &profile, "too_many_actions", 0).is_err());
        assert!(forge(&original, &key, &profile, "invalid_signature", 0).is_err());
    }

    #[test]
    fn the_arguments_are_exact() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let full = args(&[
            "ordinary-forge",
            "--variant",
            "too_many_actions",
            "--transport",
            "t",
            "--key-file",
            "k",
            "--profile",
            "p",
            "--k",
            "3",
            "--out",
            "o",
        ]);
        assert_eq!(parse(&full).unwrap().k, 3);
        let mut twice = full.clone();
        twice.extend(args(&["--k", "4"]));
        assert!(parse(&twice).is_err());
        assert!(parse(&args(&["ordinary-forge", "--variant", "wrong_chain"])).is_err());
        assert!(parse(&args(&["forge"])).is_err());
    }
}
