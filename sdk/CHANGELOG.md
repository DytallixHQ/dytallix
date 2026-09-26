# Changelog

## [Unreleased]

### Added

- Documentation hub and linked reference pages
- Example index for runnable repository flows

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
