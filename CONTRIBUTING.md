# Contributing to Dytallix

Dytallix is built by one person, and contributions are welcome. Each
component has its own guide for building, testing and documentation:

- [node](node/CONTRIBUTING.md): the consensus application, engine fork,
  supervisor and HTTP adapter
- [sdk](sdk/CONTRIBUTING.md): the Rust SDK and the `dytallix` CLI wallet
- [pqc](pqc/CONTRIBUTING.md), [contracts](contracts/CONTRIBUTING.md) and
  [docs](docs/CONTRIBUTING.md)

Report security issues privately, as the [security policy](SECURITY.md)
describes, never in a public issue.

## Sign your commits off

Every commit in a pull request from outside the project carries a
`Signed-off-by` line with your name and the address you commit with. It
certifies the [Developer Certificate of Origin](DCO): that you wrote the
change or otherwise have the right to submit it under the project's license.
`git commit -s` adds the line:

```text
Signed-off-by: Your Name <you@example.org>
```

A pull request check refuses commits without it. To sign off commits you
already made, run `git rebase --signoff main` and push again.

## License

Dytallix is dual licensed under the [MIT license](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option. Unless you
explicitly state otherwise, any contribution you intentionally submit for
inclusion, as defined in the Apache License, Version 2.0, is dual licensed
the same way, without any additional terms or conditions. Third-party code
keeps its own license: the CometBFT copy in
[node/consensus/cometbft/upstream](node/consensus/cometbft/upstream) is
Apache-2.0 with its own LICENSE and NOTICE.
