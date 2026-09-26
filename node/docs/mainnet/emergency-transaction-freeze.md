# Emergency transaction freeze

This implementation supports the approved default emergency action: freeze user transactions while consensus continues. Production activation remains disabled. A separate full-consensus-halt procedure is required when consensus, deterministic execution or cryptographic integrity is unsafe.

## Authorization and state

The optional emergency policy belongs to the root-authorized genesis configuration. It identifies the chain, release manifest, initial sequence, separate freeze and resume authority policies, explicit signature thresholds and resource limits. The release digest must match the externally verified root release manifest. Omission preserves existing configuration bytes and behavior. The local verifier configuration supplies execution paths and limits, not authority.

Version 1 binds each control to its action, sequence, exact finalized parent height and application hash, target height, chain and release. Version 2 adds the explicit bindings described below. The existing SLH-DSA-SHAKE-256s root emergency envelope signs a separately identified canonical artifact. This does not introduce a different signature scheme. Threshold verification requires distinct authorized keys. Wire data cannot select a verifier or grant itself authority.

The application verifies a control against committed policy and predecessor state. It stages the next sequence, freeze state and immutable action receipt in the existing chain write batch. The block record and committed head share that batch. There is no second replay database.

Only freeze and resume are supported. Neither action can transfer funds, mint, rewrite balances, replace keys, install a binary, change authority or stop consensus. Root governance, production custody and a full-halt operator procedure remain separate.

## Transaction behavior

A valid freeze affects every user transaction in its activation block, regardless of the control's position. Frozen user transactions must not enter fee, nonce, reservation or execution paths. An authorized resume continues to reject user transactions in its own block. User transaction processing can resume in the next block.

Only one control is admitted per block in this implementation. Both signature count and control bytes have explicit limits. Version 1 binds one exact target height. Version 2 uses a bounded height window and a verified historical finalized anchor. Numeric production limits remain unset.

A restart or elapsed time does not clear the freeze. A freeze also sets a persistent upgrade hold. Resume leaves that hold set. The separate upgrade executor permits only a freshly authorized named plan under the current emergency history. It leaves the global hold set. A new freeze prevents activation. The first executor migrates the receipt digest index inside the current candidate. It does not install an executable. See [upgrade execution](upgrade-execution.md).

An invalid control and a local verifier failure are different outcomes. Invalid authorization is rejected. Missing helper files, timeouts, local limit mismatches and unexpected helper output are infrastructure failures. They must not silently become a deterministic proposal rejection.

## Automatic transitions

The development configuration still explicitly selects ContinueExisting. The approved policy continues previously approved mandatory issuance, rewards, liabilities, maturity, recovery deadlines and finalized H+2 validator changes. The implementation keeps the configured automatic paths active while it blocks user execution. Production parameters and full transition qualification remain required.

Discretionary queued actions must remain blocked. Missing production behavior is not waived by this fixture mode. Preserve accrued liabilities and existing finalized commitments. Rejecting user transactions does not stop automatic application state changes.

## Recovery and qualification

Structural recovery validates schema, policy, sequence continuity, immutable receipt history, actual block/control linkage and unknown emergency keys. Application startup also verifies historical signatures with the real helper. State hashing alone is not signature verification. Old backups cannot be treated as evidence of unused authorization sequences.

Qualification must exercise the actual consensus application with ordinary transactions, root-authorized genesis, real SLH-DSA signatures, atomic commit failure, lost acknowledgement, exact replay and restart. Full engine/RPC, native Linux release and approved-host qualification remain separate from these local tests.

## Version 2 implementation contract

Version 2 is an explicit development configuration for qualification of the approved production requirements. It does not enable mainnet. Its policy identifies the genesis digest, authority epoch, maximum inclusive validity-window length and maximum anchor age. Each action requires three complete SLH-DSA signatures from a five-key authority set. Freeze and resume sets must have distinct identifiers and key material. Configuration cannot prove independent human custody.

The signed payload binds the committed policy digest, genesis, epoch, action, sequence, release, finalized anchor and inclusive validity window. The application reads the referenced anchor from actual committed block records, or the genesis anchor at height zero. It does not accept a caller's assertion that an anchor is finalized. The verifier signs the same height window in the root envelope. Old version 1 bytes retain their exact serialization.

Freeze binds an incident digest. Resume binds the same incident, the exact freeze receipt, the running candidate, a restored finalized checkpoint and a readiness-evidence digest. The restored checkpoint equals the signed historical anchor. It may precede inclusion while mandatory transitions continue. The maximum anchor age limits this delay. Digest validation does not establish that an incident is contained or that an evidence document is true.

Numeric limits are explicit fixture inputs. No default value becomes a production decision. Production schema acceptance, custodian records, measured timing limits and independent review remain open.
