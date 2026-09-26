#!/usr/bin/env python3
"""Independent Python reference for the Dytallix version 1 address format.

Bech32m checksum specification: https://bips.dev/350/ (BIP 350).
Public fixture bytes are not valid credentials. No signing or network calls occur.
"""
import argparse
import hashlib
import json
from pathlib import Path

CHARSET = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l'
NETWORKS = {'mainnet': (1, 'dytallix'), 'testnet': (2, 'tdytallix'), 'development': (3, 'ddytallix')}
SCHEMES = {'mldsa65': (1, 1952), 'mldsa87': (2, 2592), 'legacy_dilithium5': (0x8001, 2592)}
DOMAIN = b'DYTALLIX/ACCOUNT-ORIGIN/V1\0'
ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'crates/protocol-types/tests/fixtures/address-v1.json'


def polymod(values):
    state = 1
    generators = (0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3)
    for value in values:
        top = state >> 25
        state = ((state & 0x1ffffff) << 5) ^ value
        for index, generator in enumerate(generators):
            if (top >> index) & 1:
                state ^= generator
    return state


def encode_symbols(prefix, symbols, constant=0x2bc830a3):
    expanded = [ord(c) >> 5 for c in prefix] + [0] + [ord(c) & 31 for c in prefix]
    checksum = polymod(expanded + symbols + [0] * 6) ^ constant
    checksum_symbols = [(checksum >> (5 * shift)) & 31 for shift in range(5, -1, -1)]
    return prefix + '1' + ''.join(CHARSET[v] for v in symbols + checksum_symbols)


def symbols(data):
    bits = ''.join(f'{byte:08b}' for byte in data)
    bits += '0' * (-len(bits) % 5)
    return [int(bits[pos:pos + 5], 2) for pos in range(0, len(bits), 5)]


def address(network, account_id):
    return encode_symbols(NETWORKS[network][1], symbols(bytes([1, 1]) + account_id))


def origin_id(network, chain, scheme, public_key):
    chain_bytes = chain.encode('utf-8')
    code, expected_length = SCHEMES[scheme]
    if not 1 <= len(chain_bytes) <= 255 or len(public_key) != expected_length:
        raise ValueError('invalid origin fields')
    origin = (DOMAIN + bytes([NETWORKS[network][0]]) + len(chain_bytes).to_bytes(2, 'big')
              + chain_bytes + code.to_bytes(2, 'big') + len(public_key).to_bytes(4, 'big') + public_key)
    return hashlib.sha3_256(origin).digest()


def vectors():
    result = {'schema_version': 1, 'source': 'scripts/address_reference.py', 'encoding': [], 'origins': [], 'invalid': []}
    for network in NETWORKS:
        for name, account_id in [('zero', bytes(32)), ('ones', bytes([255]) * 32),
                                 ('sequence', bytes(range(32))), ('high_bit', bytes([128]) + bytes(31))]:
            result['encoding'].append(dict(network=network, name=name, account_id=account_id.hex(), address=address(network, account_id)))
        for scheme, (_, length) in SCHEMES.items():
            public_key = bytes(index % 251 for index in range(length))
            account_id = origin_id(network, 'dyt-address-v1-fixture', scheme, public_key)
            result['origins'].append(dict(network=network, chain_id='dyt-address-v1-fixture', algorithm=scheme,
                                          public_key_pattern='index_mod_251', public_key_len=length,
                                          account_id=account_id.hex(), address=address(network, account_id)))
    prefix = NETWORKS['mainnet'][1]
    data = bytes([1, 1]) + bytes(range(32))
    for name, payload in [('unknown_version', bytes([2, 1]) + data[2:]),
                          ('unknown_type', bytes([1, 2]) + data[2:]),
                          ('unversioned', data[2:]), ('short', data[:-1]), ('long', data + b'\0')]:
        result['invalid'].append(dict(name=name, network='mainnet', address=encode_symbols(prefix, symbols(payload))))
    padded = symbols(data)
    padded[-1] |= 1
    result['invalid'].append(dict(name='nonzero_padding', network='mainnet', address=encode_symbols(prefix, padded)))
    valid = address('mainnet', bytes(range(32)))
    result['invalid'] += [dict(name='bech32_checksum', network='mainnet', address=encode_symbols(prefix, symbols(data), 1)),
                          dict(name='uppercase', network='mainnet', address=valid.upper()),
                          dict(name='wrong_network', network='development', address=valid),
                          dict(name='checksum_error', network='mainnet', address=valid[:-1] + ('q' if valid[-1] != 'q' else 'p'))]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true', help='Regenerate the checked-in fixture explicitly')
    args = parser.parse_args()
    expected = json.dumps(vectors(), indent=2) + '\n'
    if args.write:
        FIXTURE.write_text(expected)
    elif FIXTURE.read_text() != expected:
        raise SystemExit('Address fixture differs from the independent reference')
    value = json.loads(expected)
    print(f"Address reference: {len(value['encoding'])} encodings, {len(value['origins'])} origins, {len(value['invalid'])} invalid encodings")
    print('SHA256 ' + hashlib.sha256(expected.encode()).hexdigest())


if __name__ == '__main__':
    main()
