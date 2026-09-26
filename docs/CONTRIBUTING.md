# Contributing to dytallix-docs

This repository is the canonical public documentation source for Dytallix.

## Rules For Changes

- keep commands, endpoints, and repository boundaries aligned with the public
  repos and live runtime surfaces
- do not imply crates.io publication for `dytallix-sdk` while Git is still the
  canonical install path
- do not describe the hosted `dytallix.com` frontend as a separate published
  source repo unless that becomes true

## Validation

Run the surface guard before opening a PR:

```bash
python3 scripts/check_public_surface.py
```

If you change public behavior wording, update `public-surface.json` in the same
PR when needed.