#!/usr/bin/env python3
"""Check profile and emission-config rejection before keys, signing, or RPC."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile

def check(binary):
    cases = [
        ('missing-profile', {}, 'DYT_BLOCK_PROFILE=development'),
        ('mainnet-profile', {'DYT_BLOCK_PROFILE': 'mainnet'}, 'mainnet adaptive allocation'),
        ('governance', {'DYT_BLOCK_PROFILE': 'development', 'DYT_ENABLE_GOVERNANCE': 'true'}, 'staged block hooks'),
        ('direct-funding', {'DYT_BLOCK_PROFILE': 'development', 'DYT_ENABLE_DEV_ENDPOINTS': 'true'}, 'use funded genesis'),
        ('missing-config', {'DYT_BLOCK_PROFILE': 'development', 'DYT_EMISSION_CONFIG': 'missing.json'}, 'No such file'),
        ('invalid-config', {'DYT_BLOCK_PROFILE': 'development', 'DYT_EMISSION_CONFIG': 'invalid.json'}, 'expected'),
    ]
    for name, overrides, expected in cases:
        with tempfile.TemporaryDirectory(prefix='dytallix-profile-check-') as directory:
            root = Path(directory)
            (root / 'invalid.json').write_text('invalid configuration')
            env = {'PATH': os.environ.get('PATH', '/usr/bin:/bin'),
                   'DYT_DATA_DIR': str(root / 'data'), 'DYT_KEYSTORE_DIR': str(root / 'keys'),
                   'VALIDATOR_ID': 'existing', 'DYT_REQUIRE_EXISTING_VALIDATOR_KEY': '1',
                   'DYT_RPC_PORT': '0', **overrides}
            result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, timeout=30)
            assert result.returncode == 1 and expected in result.stderr, (name, result.stderr)
            assert 'Validator key initialization failed:' not in result.stderr
            assert not (root / 'keys').exists()
            print(f'{name}: rejected before validator key access')

if __name__ == '__main__':
    check(Path(sys.argv[1]).resolve(strict=True))
