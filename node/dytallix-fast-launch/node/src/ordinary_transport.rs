//! Bounded AS06 transport decoding. This does not authenticate or admit a transaction.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use dytallix_protocol_types::{
    ordinary::{self, Limits, SignedOrdinary},
    ordinary_v3::{self as v3, SignedOrdinary as SignedOrdinaryV3, V3Limits},
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Transport {
    #[serde(rename = "type")]
    kind: String,
    envelope_base64: String,
}

/// Limits come from an explicit caller profile. No production defaults are supplied.
pub fn decode_transport(
    raw: &[u8],
    limits: &Limits,
    max_transport_bytes: usize,
) -> Result<SignedOrdinary> {
    limits.validate()?;
    ensure!(max_transport_bytes > 0, "Explicit transport limit required");
    ensure!(raw.len() <= max_transport_bytes, "Transport exceeds limit");
    let transport: Transport = serde_json::from_slice(raw).context("Invalid ordinary transport")?;
    ensure!(
        transport.kind == "ordinary_v2",
        "Unsupported transport type"
    );
    let encoded_limit = ((u64::from(limits.max_wire_bytes) + 2) / 3) * 4;
    ensure!(
        transport.envelope_base64.len() as u64 <= encoded_limit,
        "Encoded envelope exceeds limit"
    );
    let bytes = STANDARD
        .decode(&transport.envelope_base64)
        .context("Invalid ordinary envelope base64")?;
    ensure!(
        STANDARD.encode(&bytes) == transport.envelope_base64,
        "Noncanonical ordinary envelope base64"
    );
    Ok(ordinary::decode(&bytes, limits)?)
}

pub fn encode_transport(
    signed: &SignedOrdinary,
    limits: &Limits,
    max_transport_bytes: usize,
) -> Result<Vec<u8>> {
    ensure!(max_transport_bytes > 0, "Explicit transport limit required");
    let bytes = ordinary::encode(signed, limits)?;
    let raw = serde_json::to_vec(&Transport {
        kind: "ordinary_v2".into(),
        envelope_base64: STANDARD.encode(bytes),
    })?;
    ensure!(raw.len() <= max_transport_bytes, "Transport exceeds limit");
    Ok(raw)
}

/// Decode the separate v3 transport type. This authenticates no signature and
/// does not enable the v3 type in consensus admission or block execution.
pub fn decode_transport_v3(
    raw: &[u8],
    limits: &V3Limits,
    max_transport_bytes: usize,
) -> Result<SignedOrdinaryV3> {
    limits.validate()?;
    ensure!(
        max_transport_bytes > 0,
        "Explicit v3 transport limit required"
    );
    ensure!(
        raw.len() <= max_transport_bytes,
        "V3 transport exceeds limit"
    );
    let transport: Transport = serde_json::from_slice(raw).context("Invalid v3 transport")?;
    ensure!(
        transport.kind == "ordinary_v3",
        "Unsupported v3 transport type"
    );
    let encoded_limit = ((u64::from(limits.max_wire_bytes) + 2) / 3) * 4;
    ensure!(
        transport.envelope_base64.len() as u64 <= encoded_limit,
        "Encoded v3 envelope exceeds limit"
    );
    let bytes = STANDARD
        .decode(&transport.envelope_base64)
        .context("Invalid v3 envelope base64")?;
    ensure!(
        STANDARD.encode(&bytes) == transport.envelope_base64,
        "Noncanonical v3 envelope base64"
    );
    Ok(v3::decode(&bytes, limits)?)
}

pub fn encode_transport_v3(
    signed: &SignedOrdinaryV3,
    limits: &V3Limits,
    max_transport_bytes: usize,
) -> Result<Vec<u8>> {
    ensure!(
        max_transport_bytes > 0,
        "Explicit v3 transport limit required"
    );
    let bytes = v3::encode(signed, limits)?;
    let raw = serde_json::to_vec(&Transport {
        kind: "ordinary_v3".into(),
        envelope_base64: STANDARD.encode(bytes),
    })?;
    ensure!(
        raw.len() <= max_transport_bytes,
        "V3 transport exceeds limit"
    );
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Shared codec vectors contain deliberately invalid placeholder signatures.
    fn fixture() -> (SignedOrdinary, Limits) {
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../crates/protocol-types/tests/fixtures/ordinary_v2_vectors.json"
        ))
        .unwrap();
        (
            serde_json::from_value(vectors["vectors"][0]["input"].clone()).unwrap(),
            serde_json::from_value(vectors["limits"].clone()).unwrap(),
        )
    }
    fn fixture_v3() -> (SignedOrdinaryV3, V3Limits) {
        let (signed, limits) = fixture();
        let mut signed: SignedOrdinaryV3 =
            serde_json::from_value(serde_json::to_value(signed).unwrap()).unwrap();
        let data = vec![1, 2, 3];
        signed.body.actions = vec![v3::Action::GovernanceProposal {
            proposal_id: 23,
            action_class: 17,
            action_digest: v3::governance_action_digest(17, &data).unwrap(),
            action_data: data,
        }];
        (
            signed,
            V3Limits {
                ordinary: limits,
                max_governance_action_bytes: 128,
            },
        )
    }
    #[test]
    fn canonical_transport_roundtrip_and_explicit_byte_limits() {
        let (signed, limits) = fixture();
        let raw = encode_transport(&signed, &limits, 100_000).unwrap();
        assert_eq!(decode_transport(&raw, &limits, raw.len()).unwrap(), signed);
        assert!(decode_transport(&raw, &limits, raw.len() - 1).is_err());
        assert!(encode_transport(&signed, &limits, raw.len() - 1).is_err());
        assert!(encode_transport(&signed, &limits, 0).is_err());
        assert!(decode_transport(&raw, &limits, 0).is_err());
        let mut small = limits.clone();
        small.max_wire_bytes = 1;
        assert!(decode_transport(&raw, &small, raw.len()).is_err());
        let mut undefined = limits;
        undefined.max_actions = 0;
        assert!(decode_transport(&raw, &undefined, raw.len()).is_err());
    }
    #[test]
    fn duplicate_unknown_missing_fields_and_wrong_types_reject() {
        let (signed, limits) = fixture();
        let raw = encode_transport(&signed, &limits, 100_000).unwrap();
        let text = String::from_utf8(raw.clone()).unwrap();
        for suffix in [
            r#", "type":"ordinary_v2"}"#,
            r#", "envelope_base64":""}"#,
            r#", "extra":0}"#,
        ] {
            let invalid = format!("{}{}", &text[..text.len() - 1], suffix);
            assert!(decode_transport(invalid.as_bytes(), &limits, 100_000).is_err());
        }
        for invalid in [
            br#"{"type":"ordinary_v2"}"#.as_slice(),
            br#"{"envelope_base64":""}"#.as_slice(),
            br#"{"type":2,"envelope_base64":""}"#.as_slice(),
            br#"{"type":"signed","envelope_base64":""}"#.as_slice(),
            b"\xff",
        ] {
            assert!(decode_transport(invalid, &limits, 100_000).is_err());
        }
        let mut trailing = raw;
        trailing.extend_from_slice(b"{}");
        assert!(decode_transport(&trailing, &limits, 100_000).is_err());
    }
    #[test]
    fn noncanonical_base64_and_binary_envelopes_reject() {
        let (signed, limits) = fixture();
        let raw = encode_transport(&signed, &limits, 100_000).unwrap();
        let mut transport: Transport = serde_json::from_slice(&raw).unwrap();
        let original = transport.envelope_base64.clone();
        // The fixture has required padding. Missing, extra, or embedded whitespace rejects.
        assert!(original.ends_with('='));
        assert!(original.ends_with("dw=="));
        let nonzero_padding_bits = format!("{}dx==", &original[..original.len() - 4]);
        for invalid in [
            original.trim_end_matches('=').to_string(),
            format!("{original}="),
            format!(" {original}"),
            "-_==".into(),
            nonzero_padding_bits,
        ] {
            transport.envelope_base64 = invalid;
            assert!(
                decode_transport(&serde_json::to_vec(&transport).unwrap(), &limits, 100_000)
                    .is_err()
            );
        }
        let mut bytes = STANDARD.decode(original).unwrap();
        bytes.push(0);
        transport.envelope_base64 = STANDARD.encode(&bytes);
        assert!(
            decode_transport(&serde_json::to_vec(&transport).unwrap(), &limits, 100_000).is_err()
        );
        bytes.pop();
        bytes[0] ^= 1;
        transport.envelope_base64 = STANDARD.encode(&bytes);
        assert!(
            decode_transport(&serde_json::to_vec(&transport).unwrap(), &limits, 100_000).is_err()
        );
    }

    #[test]
    fn v3_transport_is_bounded_and_cannot_be_read_as_v2() {
        let (signed, limits) = fixture_v3();
        let raw = encode_transport_v3(&signed, &limits, 100_000).unwrap();
        assert_eq!(
            decode_transport_v3(&raw, &limits, raw.len()).unwrap(),
            signed
        );
        assert!(decode_transport(&raw, &limits.ordinary, raw.len()).is_err());
        assert!(decode_transport_v3(&raw, &limits, raw.len() - 1).is_err());
        assert!(encode_transport_v3(&signed, &limits, raw.len() - 1).is_err());
        let (v2_signed, v2_limits) = fixture();
        let v2_raw = encode_transport(&v2_signed, &v2_limits, 100_000).unwrap();
        assert!(decode_transport_v3(&v2_raw, &limits, v2_raw.len()).is_err());
        let mut transport: Transport = serde_json::from_slice(&raw).unwrap();
        transport.envelope_base64.push('=');
        assert!(
            decode_transport_v3(&serde_json::to_vec(&transport).unwrap(), &limits, 100_000)
                .is_err()
        );
    }
}
