# Embedded component lineage

The node workspace consumes `pqc-crypto` and `smart-contracts` from this
repository. Their standalone repositories contain different source variants.
Do not qualify an embedded component using tests of a different standalone
copy.

`component-lineage.json` records the candidate file hashes and standalone
comparison. The base commit identifies the starting revision. A candidate hash
can include local changes after that revision. It is not a published release.

For each component change:

1. Review the change in the embedded component and each shipped standalone copy.
2. Apply the correction to each affected copy without replacing unrelated code.
3. Run the affected component tests and node integration checks.
4. Review every remaining difference. Record its purpose and owner.
5. Update the candidate hashes and reference revision in the manifest.
6. Run `python3 scripts/check_component_lineage.py`.

CI rejects additions, deletions, or changed component files that lack an updated
record. Matching hashes establish source identity only. They do not prove
security, compatibility, or approval of the recorded differences.

The current record remains a local candidate. Owners must approve unresolved
differences and select the release commits before mainnet qualification.
