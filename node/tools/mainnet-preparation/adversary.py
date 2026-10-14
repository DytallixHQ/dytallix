#!/usr/bin/env python3
"""The adversarial generator (simulation plan, T4): it submits malformed and
non-canonical transactions through the client channel and checks the node
refuses each at admission while a valid control transaction is still admitted
and committed. It writes digest-bound evidence that `adversary.py verify`
re-evaluates from the file alone.

  adversary.py verify --evidence FILE --genesis NATIVE_GENESIS \
      --engine-genesis ENGINE_GENESIS

Each case records the admission code the node returned, its log, and whether the
transaction ever committed. A refuse case PASSES only when the node itself
refused it (CheckTx code greater than zero, not a transport error and not code
zero) and nothing committed; a transport or uncertain outcome never passes, so a
dropped request cannot be mistaken for a refusal. Where the refusal reason is
known it must appear in the log, so a transaction refused for the wrong reason
does not count. The positive control is the opposite: it must be admitted (code
zero) and commit, so a gateway or pin that refuses everything cannot make the
run pass.

The lean scope is the cases that do not need the load machines: an invalid
ML-DSA-65 signature, a decode failure, a stale (already spent) nonce, a
duplicate, a re-signed copy and an oversized request. Transaction and request
floods follow with the load machines (R03).

The byte-level mutations here are offline and unit-tested: a valid transport is
decoded, exactly the field under test is changed, and it is re-wrapped. The
signature is the trailing 3309 bytes of the ML-DSA-65 envelope, so flipping a
byte there changes only the signature, not the structure.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import reconcile  # noqa: E402

SCHEMA = 'dytallix.adversary-run.v1'
PASS, FAIL, INCONCLUSIVE = reconcile.PASS, reconcile.FAIL, reconcile.INCONCLUSIVE

# ML-DSA-65 signature size (sdk/vendor/dytallix-protocol-types/src/ordinary.rs:27),
# the trailing blob of an ordinary_v2 envelope (:576, :763-765).
MLDSA65_SIGNATURE_BYTES = 3309

# Cases the node refuses before CheckTx. An oversized transaction is caught by
# the mempool/RPC size limit (max_tx_bytes), returned as a JSON-RPC error, not a
# CheckTx code. That is a real refusal with a definite "too large" message, so it
# counts when its expected reason is in the log; every other case must be refused
# by CheckTx (code > 0). This set is keyed by name so a record cannot opt in.
TRANSPORT_REFUSAL_OK = {'oversized'}

# Each case's name mapped to what the node must do with it. verify recomputes
# this from the name, so a record cannot relabel a refuse case as a control.
EXPECTATION = {
    'invalid_signature': 'refuse',
    'decode_failure': 'refuse',
    'stale_nonce': 'refuse',
    'duplicate': 'refuse',
    'resigned_copy': 'refuse',
    'oversized': 'refuse',
    'positive_control': 'admit',
}


class Failed(Exception):
    pass


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def expectation(name):
    try:
        return EXPECTATION[name]
    except KeyError:
        raise Failed(f'{name!r} is not a registered adversarial case')


# Byte-level mutations on an ordinary_v2 transport blob
# ({"type":"ordinary_v2","envelope_base64": "..."}).

def _envelope(tx_bytes):
    wire = json.loads(tx_bytes)
    return wire, base64.b64decode(wire['envelope_base64'])


def corrupt_signature(tx_bytes):
    """Flip one byte inside the trailing ML-DSA-65 signature, preserving the
    envelope's length and structure, so the node fails signature verification."""
    wire, envelope = _envelope(tx_bytes)
    if len(envelope) <= MLDSA65_SIGNATURE_BYTES:
        raise Failed('the envelope is smaller than one signature')
    body = bytearray(envelope)
    at = len(body) - 16  # well inside the trailing signature blob
    body[at] ^= 0x01
    wire['envelope_base64'] = base64.b64encode(bytes(body)).decode()
    return json.dumps(wire).encode()


def corrupt_encoding(tx_bytes):
    """Break the envelope so the node cannot decode it at all."""
    wire = json.loads(tx_bytes)
    wire['envelope_base64'] = '!!not base64!!'
    return json.dumps(wire).encode()


def oversized_envelope(byte_count):
    """A transport whose envelope is larger than any profile allows, so the node
    refuses it on size before it can be anything else."""
    envelope = base64.b64encode(b'\x00' * byte_count).decode()
    return json.dumps({'type': 'ordinary_v2', 'envelope_base64': envelope}).encode()


def evaluate(case):
    """The status and detail of one case, derived only from its recorded code,
    commit outcome and log. Never reads the recorded status."""
    kind = expectation(case['name'])
    code, committed, log = case.get('code'), case.get('committed'), case.get('log') or ''
    if kind == 'admit':
        if code == 0 and committed is True:
            return PASS, 'the control transaction was admitted and committed'
        return FAIL, f'the control was not admitted and committed (code {code}, committed {committed})'
    # refuse
    if not isinstance(code, int):
        return FAIL, f'no node refusal (a transport or uncertain outcome, code {code})'
    if code == 0:
        return FAIL, 'the node admitted the transaction'
    if committed:
        return FAIL, 'the transaction committed despite admission refusal'
    reason = case.get('expected_reason')
    if code < 0:
        # Refused before CheckTx (the mempool/RPC size limit). Only for the cases
        # that are expected there, and only with the definite reason in the log.
        if case['name'] in TRANSPORT_REFUSAL_OK and reason and reason in log:
            return PASS, f'the node refused it before admission for {reason!r} (code {code})'
        return FAIL, f'no CheckTx refusal (code {code}): {log[:120]}'
    if reason and reason not in log:
        return FAIL, f'refused (code {code}) but not for the expected reason {reason!r}: {log[:120]}'
    return PASS, f'the node refused it at admission (code {code})'


def run_status(statuses):
    if FAIL in statuses:
        return FAIL
    if INCONCLUSIVE in statuses or not statuses:
        return INCONCLUSIVE
    return PASS


def build_case(case):
    row = dict(case)
    row['expectation'] = expectation(case['name'])
    row['status'], row['detail'] = evaluate(case)
    return row


def build_run(chain_id, genesis_bytes, engine_genesis_bytes, cases, host):
    rows = [build_case(case) for case in cases]
    body = {'schema': SCHEMA, 'chain_id': chain_id, 'host': host,
            'genesis': {'native_genesis_sha256': digest(genesis_bytes),
                        'engine_genesis_sha256': digest(engine_genesis_bytes)},
            'cases': rows, 'status': run_status([row['status'] for row in rows])}
    body['sha256'] = digest(canonical(body))
    return body


def verify(record, genesis_bytes, engine_genesis_bytes):
    """Re-evaluate a run record from its cases; any edit, a swapped genesis or a
    relabelled case shows."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or digest(canonical(body)) != record.get('sha256'):
        raise Failed('the evidence digest does not match its contents')
    if record['genesis']['native_genesis_sha256'] != digest(genesis_bytes):
        raise Failed('the evidence names another native genesis')
    if record['genesis']['engine_genesis_sha256'] != digest(engine_genesis_bytes):
        raise Failed('the evidence names another engine genesis')
    cases = record['cases']
    if not cases:
        raise Failed('the record holds no adversarial cases')
    names = {row['name'] for row in cases}
    if 'positive_control' not in names:
        raise Failed('the record has no positive control')
    statuses = []
    for row in cases:
        if row.get('expectation') != expectation(row['name']):
            raise Failed(f'the recorded expectation for {row["name"]!r} is wrong')
        status, detail = evaluate(row)
        if (row.get('status'), row.get('detail')) != (status, detail):
            raise Failed(f'the recorded case {row["name"]!r} does not follow from its outcome')
        statuses.append(status)
    if run_status(statuses) != record['status']:
        raise Failed('the recorded run status does not follow from its cases')
    return record['status']


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    v = commands.add_parser('verify')
    v.add_argument('--evidence', type=Path, required=True)
    v.add_argument('--genesis', type=Path, required=True, help='native-genesis.json')
    v.add_argument('--engine-genesis', type=Path, required=True, help='the engine genesis.json')
    args = parser.parse_args()
    try:
        record = json.loads(args.evidence.read_bytes())
        status = verify(record, args.genesis.read_bytes(), args.engine_genesis.read_bytes())
    except (Failed, reconcile.Failed, OSError, KeyError, ValueError) as error:
        print(f'adversary: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    raise SystemExit(main())
