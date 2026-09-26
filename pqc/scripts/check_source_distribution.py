#!/usr/bin/env python3
"""Compare component source with its recorded export hashes, not release provenance."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1]
manifest = json.loads((root / 'SOURCE-MANIFEST.json').read_text())
expected = {name: digest for name, digest in manifest['sha256'].items()
            if name.startswith(('src/', 'tests/'))}
actual = {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
          for directory in ['src', 'tests'] for path in (root / directory).rglob('*')
          if path.is_file() and '__pycache__' not in path.parts}
if actual != expected:
    raise SystemExit('Source differs from the recorded node export; regenerate and review the distribution.')
print('PQC source matches its recorded node export.')
