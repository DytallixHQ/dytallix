#!/usr/bin/env python3
"""Check funded DRT configuration before the missing-validator-key gate."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def check(binary):
    for kind in ['default', 'matching', 'mismatch']:
        with tempfile.TemporaryDirectory(prefix='dytallix-supply-startup-') as directory:
            root = Path(directory)
            (root / 'genesis.json').write_text(json.dumps({
                'chain_id': 'supply-check',
                'accounts': [{'address': 'fixture', 'balances': {'udrt': '9007199254740993'}}],
            }))
            env = {
                'PATH': os.environ.get('PATH', '/usr/bin:/bin'),
                'DYT_CHAIN_ID': 'supply-check', 'DYT_BLOCK_PROFILE': 'development',
                'DYT_DATA_DIR': str(root / 'data'), 'DYT_KEYSTORE_DIR': str(root / 'keys'),
                'VALIDATOR_ID': 'existing', 'DYT_REQUIRE_EXISTING_VALIDATOR_KEY': '1',
                'DYT_RPC_PORT': '0',
            }
            if kind != 'default':
                (root / 'emission.json').write_text(json.dumps({
                    'schedule': {'Static': {'per_block': 100}},
                    'initial_supply': 9007199254740993 if kind == 'matching' else 0,
                    'emission_breakdown': {'block_rewards': 60, 'staking_rewards': 25,
                                           'ai_module_incentives': 10, 'bridge_operations': 5},
                }))
                env['DYT_EMISSION_CONFIG'] = str(root / 'emission.json')
            expected = ('Emission initial supply differs from funded genesis' if kind == 'mismatch'
                        else 'Validator key initialization failed:')
            # The second invocation checks a restart with unchanged source bytes.
            for _ in range(2):
                result = subprocess.run([str(binary)], cwd=root, env=env,
                                        capture_output=True, text=True, timeout=30)
                assert result.returncode == 1 and expected in result.stderr, (kind, result.stderr)
                assert not (root / 'keys' / 'validator-existing.seal').exists()
            print(f'{kind}: initial startup and restart reached the expected gate')


if __name__ == '__main__':
    check(Path(sys.argv[1]).resolve(strict=True))
