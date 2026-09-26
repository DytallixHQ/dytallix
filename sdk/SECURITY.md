# Security Policy

Dytallix is a post-quantum-cryptography-native blockchain. Security is the
entire point. If you find a vulnerability, report it privately.

Do not open a public GitHub issue for security vulnerabilities.

## Reporting a Vulnerability

Report security issues privately through GitHub Security Advisories or by email
to [hello@dytallix.com](mailto:hello@dytallix.com).

If you use GitHub, open a private advisory draft from the repository's
Security tab. Use email when you need to share details before an advisory is
opened.

Include:

- A description of the vulnerability
- The affected component: `dytallix-core`, `dytallix-sdk`, `dytallix-cli`, or chain infrastructure
- Reproduction steps, proof of concept, or logs when applicable
- Your severity and exploitability assessment
- Whether you want public credit when the issue is disclosed

We aim to acknowledge receipt within 3 business days.

## What Happens Next

1. We confirm receipt and begin investigation.
2. We assess severity and decide on a disclosure timeline.
3. We develop and test a fix.
4. We publish a patched release or advisory as appropriate.
5. We credit the reporter unless they request otherwise.

## Scope

The following areas are in scope for security reports:

- `dytallix-core`: ML-DSA-65 key generation, Bech32m address derivation and validation, signature verification, BLAKE3 hashing
- `dytallix-sdk`: transaction construction and signing, faucet client behavior, keystore handling, network client behavior
- `dytallix-cli`: commands that could expose private key material, bypass address validation, or submit unauthorized transactions
- Chain-level integrations: consensus logic, networking, execution environment, fee market assumptions

## Out of Scope

- Bugs that only affect third-party dependencies and should be reported upstream
- Purely theoretical attacks with no practical exploit path
- Issues that require physical access to the device running the node

## Cryptographic Implementation Notes

Public cryptographic limitations and open problems may also be discussed in
Dytallix public materials such as the
[organization page](https://github.com/DytallixHQ) and the
[documentation repository](https://github.com/DytallixHQ/dytallix-docs).

If you believe a known limitation is more serious than characterized, or if you
find an issue not already documented, report it privately.

## Preferred Language

English.

## Disclosure Policy

We follow responsible disclosure. Please allow a reasonable window to develop
and ship a fix before public disclosure. We will work with you on timing.

Do not use Discord or public issues for vulnerability reporting.
