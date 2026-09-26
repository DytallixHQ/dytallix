#!/usr/bin/env python3
"""Exact rational reference vectors. Parameters are test data, not mainnet gains."""
from fractions import Fraction as F
from pathlib import Path
import argparse
import struct
import hashlib

S = 1_000_000

def certificate(a, b, soft, hard, window, emission_reference):
    """Sufficient target-equilibrium certificate; inputs must be exact rationals.

    a,b bound the normalized plant. This function cannot validate those bounds.
    """
    a, b, emission_reference = map(F, (a, b, emission_reference))
    if (a < 0 or b < 0 or emission_reference <= 0 or window < 1
            or len(soft) != 3 or len(hard) != 3
            or any(F(g) < 0 for g in (*soft, *hard))):
        raise ValueError('Invalid certificate domain')
    kp, ki, kd = (max(F(x), F(y))/emission_reference for x, y in zip(soft, hard))
    rho = a + b*(kp + window*ki + 2*kd)
    return rho, rho < 1

def vectors():
    rows = ['epoch\tu\tsigma\terror\tintegral\tderivative\thard\tkp\tki\tkd\traw\temission']
    history = []
    previous = 0
    observations = [(700000, 0), (600001, 0), (600000, 500000),
                    (1000000, 500001), (0, 1000000), (0, 0), (0, 0),
                    (1000000, 0), (1000000, 0), (1000000, 0),
                    (699999, 2**64 - 1), (700001, 500001), (700000, 0)]
    for epoch, (u, sigma) in enumerate(observations):
        error = F(700000 - u, S)
        history = (history + [error])[-3:]
        integral = max(F(-1, 2), min(F(1, 2), sum(history)))
        derivative = error - previous
        hard = abs(error) >= F(1, 10)
        gains = (400000, 40000, 100000) if hard else (200000, 20000, 50000)
        factor = 1 / (1 + F(sigma, S)) if sigma > 500000 else F(1)
        gains = tuple(int(gain * factor) for gain in gains)
        raw = 1400000 + sum(int(gain * value) for gain, value in zip(gains, (error, integral, derivative)))
        emission = min(2500000, max(500000, raw))
        row = [epoch, u, sigma, int(error*S), int(integral*S), int(derivative*S), int(hard), *gains, raw, emission]
        rows.append('\t'.join(map(str, row)))
        previous = error
    return '\n'.join(rows) + '\n'

def encoding_vectors():
    # Independently specified field order and integer widths. No Rust encoder runs here.
    config = struct.pack('>QQQIqqQQQQQQQQQ',
                         700000, 100000, 500000, 3, -500000, 500000,
                         200000, 20000, 50000, 400000, 40000, 100000,
                         1400000, 500000, 2500000)
    prefix = b'DYTAEC01' + struct.pack('>H', 1) + config
    rows = [(prefix + struct.pack('>BQI', 0, 0, 0)).hex()]
    errors = []
    for epoch, utilization in enumerate([700000, 600000, 1000000, 0]):
        errors = (errors + [700000 - utilization])[-3:]
        data = prefix + struct.pack('>BQI', 1, epoch, len(errors))
        data += b''.join(struct.pack('>q', error) for error in errors)
        rows.append(data.hex())
    return '\n'.join(rows) + '\n'

def journal_vectors():
    states = [bytes.fromhex(row) for row in encoding_vectors().splitlines()[:2]]
    binding = bytes([7])*32
    heads = []
    hashes = []
    for state in states:
        body = b'DYTAEH01' + binding + state
        checksum = hashlib.sha256(body).digest()
        heads.append(body + checksum)
        hashes.append(checksum)
    event = b'DYTAEJ01' + binding + struct.pack('>QQQQQ', 0, 700000, 1000000, 1, 1400000)
    event += hashes[0] + hashes[1]
    event += hashlib.sha256(event).digest()
    return '\n'.join(data.hex() for data in [*heads, event]) + '\n'

def proof_checks():
    # Numeric-domain proof uses worst-case intermediate values before division.
    max_gain = 2**64 - 1
    max_window = 65536
    bound = max_gain + max_gain*S*(max_window + 3)
    assert bound < 2**127
    assert max_window*S < 2**63
    # A conservative delayed-loop certificate, in dimensionless normalized units.
    # This is a nonempty illustrative domain, not empirical plant calibration.
    # beta=1/2, mu=eta=1, alpha=1/10, x_min=1/2 imply a=11/20,b=1.
    # Maximum normalized gains are 1/5,1/100,1/50, with three samples.
    rho, accepted = certificate(F(11, 20), 1, (100000, 5000, 10000),
                                (200000, 10000, 20000), 3, S)
    assert accepted and rho == F(41, 50)
    assert certificate(1, 0, (0, 0, 0), (0, 0, 0), 1, 1) == (F(1), False)
    # The vector fixture gains are deliberately not claimed stable under this plant bound.
    assert not certificate(F(11, 20), 1, (200000, 20000, 50000),
                           (400000, 40000, 100000), 3, S)[1]
    # Printed-model counterexample fails a necessary quadratic Jury condition.
    old_a, old_b, old_kp = F(465, 1000), F(1, 4), F(8)
    assert 1-old_a+old_b*old_kp > 0
    assert 1+old_a+old_b*old_kp > 0
    assert 1-old_b*old_kp == -1
    return {'arithmetic_bound': bound, 'illustrative_contraction_factor': str(rho),
            'plant_calibrated': False, 'mainnet_approved': False}

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    path = Path(__file__).resolve().parents[1] / 'crates/adaptive-emission/tests/vectors.tsv'
    expected = vectors()
    if args.write:
        path.write_text(expected)
    else:
        assert path.read_text() == expected, 'Reference vectors differ'
    binary_path = path.with_name('encoding-vectors.txt')
    if args.write:
        binary_path.write_text(encoding_vectors())
    else:
        assert binary_path.read_text() == encoding_vectors(), 'Encoding vectors differ'
    journal_path = path.parents[2] / 'storage/src/adaptive/journal-vectors.txt'
    if args.write:
        journal_path.write_text(journal_vectors())
    else:
        assert journal_path.read_text() == journal_vectors(), 'Journal vectors differ'
    print(proof_checks())
