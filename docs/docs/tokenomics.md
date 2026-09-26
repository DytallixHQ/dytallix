# Tokenomics

Dytallix documentation and source materials consistently describe a dual-token
system, but the current public testnet implementation has one important
short-term caveat: fees are presently charged in `DGT`.

This page separates the token roles from the current live-node behavior.

## Token Roles

### `DGT`

Current documented role:

- governance
- staking and delegation
- current public testnet fee token

Micro-denom:

- `udgt`

### `DRT`

Current documented role:

- rewards
- transferable asset on the public testnet
- staking reward and reward-token language in explorer and SDK docs

Micro-denom:

- `udrt`

## Current Public Testnet Behavior

The live `GET /status` endpoint currently reports:

- `fee_denom: "udgt"`
- `min_gas_price: 1000`

The published node source also charges upfront fees from `udgt`.

Practical integration guidance:

- budget gas in `DGT`
- treat `DRT` as a separate balance you may transfer or receive
- check `/status` before hard-coding fee assumptions

## Faucet Distribution

The public faucet reported the following limits on April 6, 2026:

- `10 DGT`
- `100 DRT`
- `60` minute cooldown
- `3` requests per hour

This matches the typical quickstart pattern where developers need both:

- `DGT` for fees
- `DRT` for transfers and reward-token testing

## Whitepaper And Design-Language Caveat

Some whitepaper and adjacent tokenomics language describes a broader long-range
economic design in which `DRT` plays a larger role in fees, rewards, burns, and
market-facing economics.

For builders, the right distinction is:

- whitepapers describe the protocol design direction
- the live public node defines current integration behavior

When they differ, use the live public node for software behavior.

## Gas Economics In Practice

The live node publishes:

- transfer base gas
- per-byte gas
- per additional signature gas
- key-value read gas
- key-value write gas

The published SDK converts that into a two-part fee estimate:

- `c_gas`
- `b_gas`

Current fee estimates are displayed in `DGT`.

## Governance And Staking

Across the codebase and companion appendix material, DGT is the token tied to:

- governance weight
- delegation
- validator stake

Appendix material also discusses:

- governance decay controls
- validator and oracle assumptions
- bounded governance parameters

Those materials are best read as protocol-design context rather than a strict
statement that every mechanism is already live in the public gateway.

## What Integrators Should Assume

Assume the following unless the public gateway changes:

- account balances expose both `udgt` and `udrt`
- fees are charged in `udgt`
- faucet requests may fund both tokens
- the dual-token model is real, but some economic details are still evolving
