#!/usr/bin/env python3
"""Check genesis validation and restart through the selected node executable.

Use private temporary state and require a missing validator key. Every run stops
before signing, networking, or RPC startup. No real keys are read or created.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def check(binary):
    with tempfile.TemporaryDirectory(prefix='dytallix-genesis-startup-') as directory:
        root = Path(directory)
        env = {
            'PATH': os.environ.get('PATH', '/usr/bin:/bin'),
            'DYT_CHAIN_ID': 'genesis-startup-check',
            'DYT_DATA_DIR': str(root / 'data'),
            'DYT_KEYSTORE_DIR': str(root / 'keys'),
            'VALIDATOR_ID': 'existing',
            'DYT_REQUIRE_EXISTING_VALIDATOR_KEY': '1',
            'DYT_RPC_PORT': '0',
                'DYT_BLOCK_PROFILE': 'development',
        }
        source = {
            'chain_id': 'genesis-startup-check',
            'accounts': [{'address': 'fixture-account', 'balances': {'udgt': '100'}}],
            'staking': {'user_delegation': {'delegator': 'fixture-account', 'amount_udgt': '40'}},
        }
        path = root / 'genesis.json'

        def run(expected):
            result = subprocess.run([str(binary)], cwd=root, env=env,
                                    capture_output=True, text=True, timeout=30)
            if result.returncode != 1 or expected not in result.stderr:
                raise RuntimeError(f'Expected startup failure: {expected}; got {result.stderr}')
            assert not (root / 'keys' / 'validator-existing.seal').exists()

        source['staking']['user_delegation']['amount_udgt'] = '101'
        path.write_text(json.dumps(source))
        run("Genesis stake exceeds its account's DGT allocation")
        print('unfunded stake: startup rejected before key initialization')
        source['staking']['user_delegation']['amount_udgt'] = '40'
        path.write_text(json.dumps(source))
        run('Validator key initialization failed:')
        run('Validator key initialization failed:')
        print('funded genesis: initialization and restart reached the existing-key gate')
        path.write_text(path.read_text() + '\n')
        run('Genesis source differs from initialized storage')
        print('changed genesis bytes: restart rejected')


if __name__ == '__main__':
    if len(sys.argv) != 2:
        raise SystemExit('usage: check_genesis_startup.py <default-backend-node-binary>')
    check(Path(sys.argv[1]).resolve(strict=True))
