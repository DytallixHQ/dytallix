#!/usr/bin/env python3
"""Independent generator for ordinary_v2_vectors.json and ordinary_v3_vectors.json.

A Python struct big-endian encoder and hashlib.sha3_256 following the
ordinary account-transaction codec (versions 2 and 3), the governance action
digest and the ordinary-v3 fee profile, sharing no code with the Rust
implementation, so the fixtures cross-check it (E04 gap 8). The v1 fee
profile bytes inside a v3 profile come from generate_ordinary_fee_vectors.py.

  --check FILE   re-derive every expected value of a v2 or v3 fixture
  --write-v3 FILE  write the v3 fixture from its fixed synthetic inputs
"""

import argparse
import hashlib
import json
import os
import struct
import sys

# Keep the fixture directory free of bytecode: it is vendored byte for byte.
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from generate_ordinary_fee_vectors import encode as fee_v1_encode  # noqa: E402

SIGNING_PREFIX = b"DYTALLIX/ACCOUNT-TRANSACTION\0"
WIRE_PREFIX = b"DYTALLIX/ACCOUNT-WIRE\0"
KEY_PREFIX = b"DYTALLIX/ACCOUNT-KEY\0"
GOVERNANCE_PREFIX = b"DYTALLIX/GOVERNANCE-ACTION-V1\0"
FEE_V3_PREFIX = b"DYTALLIX/ORDINARY-FEE-PROFILE-V3\0"
FEE_V3_FORMAT = 1
FEE_V3_CONTRACT = 2
ALGORITHMS = {"mldsa65": (1, 1952, 3309), "mldsa87": (2, 2592, 4627)}
DENOMINATIONS = {"udgt": 1, "udrt": 2}
VOTES = {"yes": 1, "no": 2, "no_with_veto": 3, "abstain": 4}


def sha3(data):
    return hashlib.sha3_256(data).digest()


class Out:
    def __init__(self):
        self.b = bytearray()

    def raw(self, data):
        self.b.extend(bytes(data))

    def num(self, fmt, value):
        self.b.extend(struct.pack(">" + fmt, int(value)))

    def u128(self, value):
        self.b.extend(int(value).to_bytes(16, "big"))

    def text(self, value):
        data = value.encode("utf-8")
        assert data, "empty identifier"
        self.num("H", len(data))
        self.raw(data)

    def blob(self, data):
        self.num("I", len(data))
        self.raw(data)

    def key(self, key):
        code, size, _ = ALGORITHMS[key["algorithm"]]
        assert len(key["public_key"]) == size, "public key length"
        self.num("H", code)
        self.blob(bytes(key["public_key"]))


def governance_digest(action_class, data):
    return sha3(GOVERNANCE_PREFIX + struct.pack(">HI", action_class, len(data)) + bytes(data))


def action(out, a, version):
    kind = a["type"]
    if kind == "Send":
        out.num("B", 1)
        out.raw(a["recipient"])
        out.num("B", DENOMINATIONS[a["denomination"]])
        out.u128(a["amount"])
    elif kind == "Data":
        out.num("B", 2)
        out.blob(a["data"].encode("utf-8"))
    elif kind == "DmsRegister":
        out.num("B", 3)
        out.raw(a["beneficiary"])
        out.num("Q", a["period_blocks"])
    elif kind == "DmsPing":
        out.num("B", 4)
    elif kind == "DmsClaim":
        out.num("B", 5)
        out.raw(a["owner"])
        out.num("Q", a["expected_grant_generation"])
    elif kind in ("RewardBond", "RewardBeginUnbond"):
        out.num("B", 6 if kind == "RewardBond" else 7)
        out.text(a["validator_id"])
        out.u128(a["amount_udgt"])
    elif kind == "RewardClaim":
        out.num("B", 8)
    elif kind in ("ValidatorRegister", "ValidatorRotateKey"):
        out.num("B", 9 if kind == "ValidatorRegister" else 10)
        out.text(a["validator_id"])
        out.blob(bytes(a["consensus_key"]))
        out.blob(bytes(a["proof"]))
        out.num("Q", a["proof_expiry_height"])
        if kind == "ValidatorRegister":
            out.u128(a["amount_udgt"])
    elif kind == "ValidatorExit":
        out.num("B", 11)
        out.text(a["validator_id"])
    elif kind == "ValidatorWithdraw":
        out.num("B", 12)
        out.text(a["unbond_id"])
    elif version >= 3 and kind == "GovernanceProposal":
        data = bytes(a["action_data"])
        assert bytes(a["action_digest"]) == governance_digest(a["action_class"], data)
        out.num("B", 13)
        out.num("Q", a["proposal_id"])
        out.num("H", a["action_class"])
        out.blob(data)
        out.raw(a["action_digest"])
    elif version >= 3 and kind == "GovernanceDeposit":
        out.num("B", 14)
        out.num("Q", a["proposal_id"])
        out.u128(a["amount_udgt"])
    elif version >= 3 and kind == "GovernanceVote":
        out.num("B", 15)
        out.num("Q", a["proposal_id"])
        out.num("B", VOTES[a["choice"]])
    else:
        raise ValueError(f"action {kind} is not in version {version}")


def signing_bytes(body, version):
    out = Out()
    out.raw(SIGNING_PREFIX)
    out.num("H", version)
    domain = body["domain"]
    out.num("B", domain["network"])
    out.text(domain["chain_id"])
    out.raw(domain["genesis_digest"])
    out.raw(domain["account_id"])
    out.num("Q", body["authorization_generation"])
    out.num("Q", body["spending_nonce"])
    out.key(body["key"])
    out.num("Q", body["expiry_height"])
    out.num("H", body["ordinary_fee_contract_version"])
    out.num("Q", body["fee_profile_version"])
    out.raw(body["fee_profile_digest"])
    assert body["fee_denomination"] == "udrt"
    out.num("B", DENOMINATIONS["udrt"])
    out.u128(body["maximum_fee"])
    out.num("Q", body["gas_limit"])
    out.blob(body["memo"].encode("utf-8"))
    out.num("H", len(body["actions"]))
    for a in body["actions"]:
        action(out, a, version)
    return bytes(out.b)


def envelope(signed, version):
    body = signing_bytes(signed["body"], version)
    signature = bytes(signed["signature"])
    assert len(signature) == ALGORITHMS[signed["body"]["key"]["algorithm"]][2]
    out = Out()
    out.raw(WIRE_PREFIX)
    out.num("H", version)
    out.blob(body)
    out.blob(signature)
    return body, bytes(out.b)


def key_id(key):
    out = Out()
    out.raw(KEY_PREFIX)
    out.key(key)
    return sha3(bytes(out.b))


def fee_v3_bytes(profile):
    base, _ = fee_v1_encode(profile["base"], 2)
    out = Out()
    out.raw(FEE_V3_PREFIX)
    out.num("H", FEE_V3_FORMAT)
    out.num("H", FEE_V3_CONTRACT)
    out.num("Q", profile["version"])
    out.num("Q", profile["activation_height"])
    out.num("I", profile["max_governance_action_bytes"])
    for cost in profile["governance_action_costs"]:
        out.num("Q", cost)
    out.blob(base)
    return bytes(out.b)


def expected(signed, version):
    body, env = envelope(signed, version)
    return {
        "expected_signing_bytes_hex": body.hex(),
        "expected_envelope_hex": env.hex(),
        "expected_transaction_id": sha3(body).hex(),
        "expected_envelope_hash": sha3(env).hex(),
        "expected_key_id": key_id(signed["body"]["key"]).hex(),
    }


def version_of(fixture):
    return 3 if "fee_profile" in fixture else 2


def check(path):
    fixture = json.load(open(path))
    version = version_of(fixture)
    if version == 3:
        raw = fee_v3_bytes(fixture["fee_profile"]["input"])
        assert raw.hex() == fixture["fee_profile"]["expected_profile_hex"], "fee profile bytes"
        assert sha3(raw).hex() == fixture["fee_profile"]["expected_digest"], "fee profile digest"
    for vector in fixture["vectors"]:
        for name, value in expected(vector["input"], version).items():
            assert vector[name] == value, f"{vector['name']}: {name}"
        for a in vector["input"]["body"]["actions"]:
            if a["type"] == "GovernanceProposal":
                digest = governance_digest(a["action_class"], bytes(a["action_data"])).hex()
                assert vector["expected_governance_action_digest"] == digest, vector["name"]
    print(f"{os.path.basename(path)}: {len(fixture['vectors'])} vectors match")


def write_v3(path):
    here = os.path.dirname(os.path.abspath(__file__))
    v2 = json.load(open(os.path.join(here, "ordinary_v2_vectors.json")))
    fees = json.load(open(os.path.join(here, "ordinary_fee_v1_vectors.json")))
    profile = {
        "base": fees["profile"],
        "version": "9",
        "activation_height": "30",
        "max_governance_action_bytes": 128,
        "governance_action_costs": ["31", "32", "33"],
    }
    raw = fee_v3_bytes(profile)
    digest = sha3(raw)
    action_data = list(b"parameter:max_active=16")
    proposal = {
        "type": "GovernanceProposal",
        "proposal_id": "7",
        "action_class": 1,
        "action_data": action_data,
        "action_digest": list(governance_digest(1, bytes(action_data))),
    }
    cases = [
        ("proposal_mldsa65", 0, proposal),
        ("deposit_mldsa87", 1, {"type": "GovernanceDeposit", "proposal_id": "7", "amount_udgt": "250"}),
        ("vote_no_with_veto_mldsa65", 0, {"type": "GovernanceVote", "proposal_id": "7", "choice": "no_with_veto"}),
    ]
    vectors = []
    for name, source, governance_action in cases:
        signed = json.loads(json.dumps(v2["vectors"][source]["input"]))
        body = signed["body"]
        body["ordinary_fee_contract_version"] = FEE_V3_CONTRACT
        body["fee_profile_version"] = profile["version"]
        body["fee_profile_digest"] = list(digest)
        body["gas_limit"] = "100"
        body["maximum_fee"] = "200"
        body["actions"] = [governance_action]
        vector = {"name": name, "input": signed, **expected(signed, 3)}
        if governance_action["type"] == "GovernanceProposal":
            vector["expected_governance_action_digest"] = bytes(proposal["action_digest"]).hex()
        vectors.append(vector)
    fixture = {
        "schema_version": 1,
        "scope": "CODEC_ONLY_SYNTHETIC_PUBLIC_DATA",
        "signature_status": v2["signature_status"],
        "production_activation": False,
        "generator": ("Independent Python struct big-endian encoder and hashlib.sha3_256 following the "
                      "ordinary-v3 codec, without Rust codec output: generate_ordinary_vectors.py."),
        "limits": {"ordinary": v2["limits"], "max_governance_action_bytes": 128},
        "fee_profile": {"input": profile, "expected_profile_hex": raw.hex(), "expected_digest": digest.hex()},
        "vectors": vectors,
    }
    with open(path, "w") as handle:
        json.dump(fixture, handle, indent=2)
        handle.write("\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", metavar="FILE")
    mode.add_argument("--write-v3", metavar="FILE")
    args = parser.parse_args()
    if args.write_v3:
        write_v3(args.write_v3)
    else:
        check(args.check)


if __name__ == "__main__":
    main()
