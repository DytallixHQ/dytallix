# Security policy

Report security issues privately. Do not open a public issue, pull request
or discussion for a vulnerability, and do not post it on Discord.

## Reporting

- Use GitHub's private vulnerability reporting on this repository
  ("Report a vulnerability" under the Security tab), or
- email hello@dytallix.com.

Include a description, the affected component, reproduction steps or a proof
of concept when you have one, your severity assessment, and whether you want
public credit after disclosure. We aim to acknowledge new reports within 3
business days.

## Scope

Everything in this repository: the consensus application and engine fork
([node](node/)), the supervisor, HTTP adapter and release tooling, the SDK and
`dytallix` CLI wallet ([sdk](sdk/)), the genesis and launch tooling, and
signing, key handling and signature verification anywhere in them. Bugs in
third-party code, including the CometBFT copy in
`node/consensus/cometbft/upstream`, go to its upstream project unless our
changes or our use cause them.

Out of scope: purely theoretical issues with no practical exploit path, and
issues that need physical access to a host.

## Bounty

The frozen release gets a 30-day public review with a bug bounty paid in DGT,
and the launch is labeled unaudited until an independent human audit is
funded ([trust model](launch/TRUST_MODEL.md)). The bounty's terms are
published with the review.

## Disclosure

We follow coordinated disclosure: give us a reasonable window to investigate,
fix and release before you publish.
