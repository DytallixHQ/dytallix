#!/usr/bin/env python3
"""Independent generator for ordinary_fee_v1_vectors.json.

A Python struct encoder and hashlib.sha3_256 following the ordinary fee
profile codec, sharing no code with the Rust implementation, so the fixture
cross-checks it. `--check FILE [--format N]` re-derives an existing fixture's bytes and
digests; `--write FILE` rewrites it in the current format.
"""

import argparse
import copy
import hashlib
import json
import struct

PREFIX = b"DYTALLIX/ORDINARY-FEE-PROFILE\0"
CODES = {"mldsa65": 1, "mldsa87": 2}
FORMAT = 2
CREATION_FEE = "2500"


def encode(p, fmt):
    """Canonical profile bytes plus each field's (offset, length)."""
    out = bytearray(PREFIX)
    offsets = {}

    def put(pointer, fmt_char, value):
        offsets[pointer] = [len(out), struct.calcsize(">" + fmt_char)]
        out.extend(struct.pack(">" + fmt_char, value))

    def put_u128(pointer, value):
        offsets[pointer] = [len(out), 16]
        out.extend(int(value).to_bytes(16, "big"))

    def put_costs(name, costs):
        offsets[f"/{name}/count"] = [len(out), 1]
        out.append(len(costs))
        for algorithm in sorted(costs):
            put(f"/{name}/{algorithm}/code", "H", CODES[algorithm])
            put(f"/{name}/{algorithm}", "Q", int(costs[algorithm]))

    out.extend(struct.pack(">H", fmt))
    out.extend(struct.pack(">H", p["ordinary_fee_contract_version"]))
    put("/version", "Q", int(p["version"]))
    put("/activation_height", "Q", int(p["activation_height"]))
    assert p["denomination"] == "udrt"
    put("/denomination", "B", 2)
    for field in ("gas_price", "minimum_gas", "max_transaction_gas", "max_block_transaction_gas",
                  "max_block_transaction_bytes", "max_block_signature_checks"):
        put(f"/{field}", "Q", int(p[field]))
    put_u128("/max_fee_cap", p["max_fee_cap"])
    limits = p["limits"]
    put("/limits/max_wire_bytes", "I", limits["max_wire_bytes"])
    put("/limits/max_actions", "H", limits["max_actions"])
    put("/limits/max_identifier_bytes", "H", limits["max_identifier_bytes"])
    for field in ("max_data_bytes", "max_memo_bytes", "max_consensus_key_bytes", "max_proof_bytes"):
        put(f"/limits/{field}", "I", limits[field])
    put("/limits/max_expiry_lifetime", "Q", int(limits["max_expiry_lifetime"]))
    for field in ("transaction_overhead", "receipt_metadata_cost", "wire_byte_cost",
                  "read_byte_cost", "write_byte_cost"):
        put(f"/{field}", "Q", int(p[field]))
    for i, cost in enumerate(p["action_costs"]):
        put(f"/action_costs/{i}", "Q", int(cost))
    put_costs("signature_costs", p["signature_costs"])
    offsets["/validator_proof_profile_digest"] = [len(out), 32]
    out.extend(bytes(p["validator_proof_profile_digest"]))
    put_costs("validator_proof_costs", p["validator_proof_costs"])
    if fmt >= 2:
        put_u128("/account_creation_fee_udrt", p["account_creation_fee_udrt"])
    return bytes(out), offsets


def set_pointer(document, pointer, value):
    parts = pointer.strip("/").split("/")
    target = document
    for part in parts[:-1]:
        target = target[int(part)] if isinstance(target, list) else target[part]
    last = parts[-1]
    if isinstance(target, list):
        target[int(last)] = value
    else:
        target[last] = value


def cases(fixture, fmt):
    profile = fixture["profile"]
    for mutation in fixture["mutations"]:
        changed = copy.deepcopy(profile)
        set_pointer(changed, mutation["pointer"], mutation["value"])
        raw, _ = encode(changed, fmt)
        yield mutation, raw


def check(path, fmt):
    fixture = json.load(open(path))
    raw, _ = encode(fixture["profile"], fmt)
    assert raw.hex() == fixture["expected_profile_hex"], "profile bytes differ"
    assert hashlib.sha3_256(raw).hexdigest() == fixture["expected_digest"], "profile digest differs"
    for mutation, raw in cases(fixture, fmt):
        assert raw.hex() == mutation["expected_profile_hex"], mutation["pointer"]
        assert hashlib.sha3_256(raw).hexdigest() == mutation["expected_digest"], mutation["pointer"]


def write(path):
    fixture = json.load(open(path))
    fixture["profile"]["account_creation_fee_udrt"] = CREATION_FEE
    if not any(m["pointer"] == "/account_creation_fee_udrt" for m in fixture["mutations"]):
        fixture["mutations"].append({"pointer": "/account_creation_fee_udrt", "value": "2501"})
    raw, offsets = encode(fixture["profile"], FORMAT)
    fixture["expected_profile_hex"] = raw.hex()
    fixture["expected_digest"] = hashlib.sha3_256(raw).hexdigest()
    fixture["offsets"] = offsets
    for mutation, changed in cases(fixture, FORMAT):
        offset, length = offsets[mutation["pointer"]]
        mutation.update({"offset": offset, "length": length, "expected_profile_hex": changed.hex(),
                         "expected_digest": hashlib.sha3_256(changed).hexdigest()})
    fixture["generator"] = ("Independent Python struct encoder and hashlib.sha3_256 following OF01 "
                            "(format 2), without Rust codec output: generate_ordinary_fee_vectors.py.")
    with open(path, "w") as handle:
        json.dump(fixture, handle, indent=2)
        handle.write("\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", metavar="FILE")
    mode.add_argument("--write", metavar="FILE")
    parser.add_argument("--format", type=int, default=FORMAT, help="format of the checked file")
    args = parser.parse_args()
    if args.write:
        write(args.write)
    else:
        check(args.check, args.format)
        print("vectors match")


if __name__ == "__main__":
    main()
