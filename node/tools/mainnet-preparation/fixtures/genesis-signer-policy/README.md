# Genesis signer policy fixture

Five SLH-DSA-SHAKE-256s public key records, as `dytallix-root-sign keygen`
writes them, from disposable test keys whose private halves were never kept
with the repository. `policy.json` is what `dytallix-root-sign policy
-chain-id dytallix-staging-1` writes from them.

The genesis signer intake checker (`genesis_signer_intake.py`) must emit the
same bytes from the same records, and the Go signer must keep writing them.
These keys sign nothing and belong to no person or chain.
