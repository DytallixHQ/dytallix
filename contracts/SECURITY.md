# Security Policy

## Reporting

Do not open a public issue for exploitable vulnerabilities, cryptographic weaknesses, consensus-impacting bugs, or anything that could put contracts or validator infrastructure at risk.

Report security issues privately to the Dytallix team. If you do not already have a direct maintainer channel, send a report to `hello@dytallix.com` with the subject line `SECURITY: dytallix-contracts`.

## What To Include

- affected file paths or modules
- a concise description of the impact
- reproduction steps or a proof of concept
- any assumptions about deployment or chain state
- suggested mitigations if you have them

## Scope

High-priority reports include:

- signature verification or algorithm-registry bypasses
- reward-accounting or staking-balance corruption
- governance execution bypasses
- unauthorized mint, burn, or transfer paths in token modules
- runtime sandbox escapes or storage isolation failures
- bridge or oracle logic that can forge state transitions

## Handling

The repository aims for:

- initial acknowledgement within 3 business days
- ongoing coordination on severity and remediation
- public disclosure after a fix is available or the issue is otherwise mitigated

