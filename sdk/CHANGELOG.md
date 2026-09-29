# Changelog

## [Unreleased]

### Added

- Documentation hub and linked reference pages
- Example index for runnable repository flows
- `dytallix gateway`, the local browser companion (E04 gap 19): it relays a
	page's JSON-RPC from a loopback address to the pinned chain, over the
	client channel or loopback HTTP, and serves a wallet bundle pinned by its
	manifest digest; `gateway bundle-digest` computes the digest

### Changed

- README navigation and install guidance now point to Git-based SDK installs
- Cargo package metadata now includes homepage and documentation links
- Example headers now use the correct `first-transaction` and `deploy-contract`
	commands
- Onboarding docs now distinguish the working funded-wallet and transaction
	flow from contract deploy, which still requires an endpoint that accepts
	`POST /contracts/deploy`
- CLI contract writes now explain when the public website gateway does not
	expose `/contracts/deploy` or `/contracts/call` and how to switch to a direct
	node endpoint
- Public smoke now validates the supported contract build path instead of
	assuming the public website gateway already forwards contract write routes

### Removed

- The legacy public-testnet client, so the SDK and CLI carry no classical
	public-key cryptography and no TLS (E04 gap 19): the SDK `network` feature
	with `client` and `faucet`, and the errors `FaucetRateLimited`,
	`FaucetUnavailable`, `NodeUnavailable` and `ContractDeployFailed`; the CLI
	`legacy-network` feature with `init`, `faucet`, `contract`, `node`,
	`chain`, `dev` and `legacy`; `config set`, `config network`, `config reset`
	and `~/.dytallix/config.json`; the `first-transaction` and
	`deploy-contract` examples, the minimal contract, and `start-local.sh` and
	`stop-local.sh`. The CLI reaches the consensus chain over loopback HTTP or
	the post-quantum client channel
