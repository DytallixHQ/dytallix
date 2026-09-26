#!/usr/bin/env python3
"""Check failure before signing or RPC startup with isolated temporary state."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def check(binary):
    for label, original in [('missing', None), ('unsupported', b'legacy fixture, not a private key')]:
        with tempfile.TemporaryDirectory(prefix='dytallix-key-startup-') as directory:
            root = Path(directory)
            keys = root / 'keys'
            keys.mkdir()
            key = keys / 'validator-existing.seal'
            if original is not None:
                key.write_bytes(original)
            env = {
                'PATH': os.environ.get('PATH', '/usr/bin:/bin'),
                'DYT_CHAIN_ID': 'dyt-key-startup-check',
                'DYT_DATA_DIR': str(root / 'data'),
                'DYT_KEYSTORE_DIR': str(keys),
                'VALIDATOR_ID': 'existing',
                'DYT_REQUIRE_EXISTING_VALIDATOR_KEY': '1',
                'DYT_RPC_PORT': '0',
                'DYT_BLOCK_PROFILE': 'development',
            }
            result = subprocess.run(
                [str(binary)], cwd=root, env=env,
                capture_output=True, text=True, timeout=30,
            )
            if result.returncode != 1 or 'Validator key initialization failed:' not in result.stderr:
                raise RuntimeError(f'{label}: startup did not stop at key initialization')
            if original is None:
                assert not key.exists(), 'startup generated a missing key'
            else:
                assert key.read_bytes() == original, 'startup replaced an existing key'
            print(f'{label}: startup failed and key state remained unchanged')


if __name__ == '__main__':
    if len(sys.argv) != 2:
        raise SystemExit('usage: check_existing_key_startup.py <default-backend-node-binary>')
    check(Path(sys.argv[1]).resolve(strict=True))
