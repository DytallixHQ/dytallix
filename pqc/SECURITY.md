# Security Policy

This repository contains post-quantum cryptographic code. Report security
issues privately.

Do not open a public GitHub issue for security vulnerabilities.

## Reporting A Vulnerability

Report vulnerabilities privately by email:

- email: hello@dytallix.com

If GitHub Security Advisories are enabled on this repository, you may use that
channel instead.

Include:

- the affected primitive or module
- a clear description of the issue
- reproduction steps or proof of concept when available
- your assessment of severity and exploitability
- whether you want public credit after disclosure

We aim to acknowledge new reports within 3 business days.

## Scope

The following areas are in scope:

- signature key generation, signing, and verification
- key exchange generation and encapsulation flows
- key serialization and persistence behavior
- bridge signing and replay-protection logic
- benchmarking helpers when they affect cryptographic correctness

## Disclosure

We follow responsible disclosure and ask for a reasonable window to investigate
and ship a fix before public disclosure.

Do not use Discord or public issue threads for vulnerability reporting.
