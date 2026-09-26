# Contributing to dytallix-faucet

This repository is the public backend source for the live Dytallix faucet
flow.

## Before Opening A PR

- keep changes scoped to the faucet backend or its published edge compatibility
  config
- include the smallest automated check that proves the change
- update README or `public-capabilities.json` when public behavior changes

## Validation

Run the local checks that match your change:

```bash
npm ci
npm test -- --runInBand
node --check src/server.js
node --check src/controllers/faucetController-dual.js
```

## Security

Do not open public issues for vulnerabilities. Follow [SECURITY.md](SECURITY.md).