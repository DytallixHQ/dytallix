# Dytallix mainnet readiness

Current source: [LAUNCH_GATES.json](LAUNCH_GATES.json). The [generated master list](MAINNET_GATE_MASTER.md) records requirements, evidence and remaining work.

| Measure | Current result |
|---|---|
| Launch decision | NO GO |
| PARTIAL gates | 35 |
| OPEN / READY FOR ACCEPTANCE / PASS gates | 0 / 0 / 0 |
| Production records accepted | 0/8 |
| Final simulations completed | 0/7 |
| Decision questions OPEN / PARTIALLY_APPROVED | 29 / 5 |
| Policy questions fully open / partly approved | 21 / 5 |
| Launch preparation estimate, excluding seven final simulations | About 62%; planning range 50–70% |

The [current estimate](decision-register/emergency-upgrade-execution/ASSESSMENT.md) uses the same six weights as the previous assessment. The trust-control judgment increases for version 2 emergency controls and a real upgrade executor. The weighted estimate changes from 60.5% to 61.5%, rounded to about 62%. This is a subjective planning judgment. It is not measured effort, a completion date or a gate acceptance ratio. The earlier unsupported 70% remains withdrawn.

The [latest queue](decision-register/emergency-upgrade-execution/REPORT.md) implemented signed emergency windows, stored anchors, explicit authority epochs and evidence-bound resume. It also implemented upgrade admission, activation and cancellation. The first migration builds a persistent emergency receipt index. Migration writes and control state commit in the same database batch. Queries use the index and later receipts maintain it.

All 458 distinct Rust tests passed: 448 library tests and 10 process-interface tests. Ten signing-helper checks and 22 custody-validator tests also passed. Internal review resolved three findings and found no remaining blocking implementation finding within its scope. This is not independent security acceptance.

The migration targets the root-bound running candidate. Future releases must preserve its implementation, digest, encoding and replay behavior. Cross-binary installation, state reopening and rollback remain unqualified. Production emergency and upgrade configuration remain disabled.

The prior queue's Linux builds and 50 native lifecycle assertions remain evidence for those pinned artifacts. They predate the changed Rust code. Rebuild and inspect the current Linux candidate, then repeat affected checks through the actual consensus engine and RPC. No remote command ran in this queue. Existing websites were not changed.

D11-Q03 retains the six approved emergency recommendations. No policy approval changed. Custodian names, keys, epochs, numeric limits, production format acceptance and remaining authority inputs remain open. The [public intake packet](decision-register/emergency-upgrade-execution/custody/INTAKE.md) is ready for five custodian records. Rick Glenn remains a prior nominee; no emergency appointment is inferred.

The production packet still contains 112 populated fields and 181 unset fields across eight records. No new field or record was accepted. The dependency groups remain 54 policy/configuration inputs, 112 human custody/identity/acceptance inputs, 12 executed qualification inputs and three final-candidate inputs. Nineteen other fields require later candidate binding.

Next: qualify migration-version preservation and cross-binary upgrades; rebuild and qualify the current Linux/native stack; complete custody, economic, validator and topology inputs. Hosted endpoint trust, independent reproduction/review, production recovery, capacity, monitoring and operator acceptance remain required. Final candidate acceptance and the seven launch simulations follow those prerequisites.

G35 remains a mandatory veto. Mainnet activation remains unauthorized.
