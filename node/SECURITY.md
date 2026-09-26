# Security Policy

This repository contains public node, RPC, consensus, runtime, and
post-quantum-cryptography-related code for Dytallix. Report security issues
privately.

Do not open a public GitHub issue for security vulnerabilities.

## Reporting A Vulnerability

Report vulnerabilities privately by email:

- email: hello@dytallix.com

If GitHub Security Advisories are enabled on this repository, you may use that
channel instead.

Include:

- a description of the issue
- the affected component or package
- reproduction steps or proof of concept when available
- your severity assessment
- whether you want public credit after disclosure

We aim to acknowledge new reports within 3 business days.

## Scope

The following areas are in scope:

- `dytallix-fast-launch/node`
- `blockchain-core`
- `pqc-crypto`
- `smart-contracts`
- public RPC surfaces
- secrets-loading and signing flows

## Out Of Scope

- purely theoretical issues with no practical exploit path
- bugs that only exist in third-party dependencies and should be reported
  upstream
- issues requiring physical access to the deployment environment

## Disclosure

We follow responsible disclosure and ask for a reasonable window to investigate,
fix, and publish before public disclosure.

Do not use Discord or public issue threads for vulnerability reporting.
