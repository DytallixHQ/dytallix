# Full consensus halt procedure — draft

This is a preparation document. It does not authorize a live halt, restart, key operation, rollback or state migration. Named authorities, thresholds, communication channels and checkpoint acceptance remain required inputs.

Use the separate full-halt procedure when continued consensus, deterministic execution or cryptographic verification is unsafe. A transaction freeze cannot repair those conditions. Do not depend on a new transaction finalizing after consensus has stopped.

## Required sequence

1. Record the incident, affected candidate hashes and evidence. Use the approved independent emergency communication channel. Identify which safety assumption is uncertain.
2. Obtain the required halt authorization and coordinate the affected validator operators. If operators must stop their own unsafe signing process immediately, record that action separately from a network-wide authorization.
3. Fence each stopped signer. Preserve its last signing height, round, step, sign bytes and signature. Preserve the application database and engine records. Do not reset signing state or start a replacement signer concurrently.
4. Identify the last trustworthy finalized checkpoint from independently checked commit evidence. Record disagreement explicitly. Do not select a checkpoint solely because one node reports it.
5. Diagnose and correct the cause. Bind the correction to reviewed source, executable hashes, cryptographic profile, state compatibility and test evidence. Keep the previous records available for review.
6. Obtain separate resume authorization bound to the accepted checkpoint, release and recovery evidence. A prior halt signature does not authorize resumption, replacement keys, spending or state rewriting.
7. Verify all intended operators have the same accepted checkpoint and release. Verify exclusive signer ownership and compatible signing state before enabling signing.
8. Apply the approved restart barrier and coordinated start procedure. If a transaction freeze was not finalized before the halt, do not invent one in the database. The restart barrier requires its own reviewed mechanism.
9. Verify consistent finalized state, peer admission, signer exclusivity and required monitoring. Resolve any transaction freeze only through its separate authorized resume action.

Do not create a replacement genesis, change chain identity or revert finalized history as an automatic recovery step. Such changes are outside this procedure and require explicit decisions. No automatic timeout authorizes restart.

## Remaining inputs

Specify halt and resume controllers, thresholds, communication authentication, checkpoint verification rules, signer-fencing evidence, restart barrier implementation and failure criteria. Exercise the procedure with disposable local or assigned staging systems before final acceptance. Production private keys do not belong in the evidence package.
