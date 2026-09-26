# Native token unit contract

The user approved six decimal places for DGT and DRT during Batch 6. The fixed total of 1,000,000,000 DGT therefore equals `1000000000000000` udgt. This approval does not authorize a mint, select full-genesis issuance policy, or approve a production genesis.

| Token | Decimal places | Base denomination | Base units per token |
|---|---:|---|---:|
| DGT | 6 | udgt | 1000000 |
| DRT | 6 | udrt | 1000000 |

`crates/protocol-types/src/units.rs` defines the token scales, base denominations, and DGT total. Decimal text conversion rejects excess precision, invalid syntax, and overflow. Whole-token conversion uses checked multiplication. Neither conversion rounds an amount.

The selected node's `normalize_send` uses these scales for DGT and DRT whole-token aliases. Existing base-unit inputs retain their value. Repeated normalization does not multiply a canonical amount again. Stored transfers remain denominated in udgt or udrt.

The selected native genesis importer accepts base denominations only. Amounts must be integer digit strings. It rejects whole-token aliases instead of inferring a scale. It retains its existing DGT cap check and checked balance totals. Its integration test confirms that the cap equals the approved DGT base-unit total.

The native genesis fixture converts explicit whole-token amounts for the 30/20/15/15/20 allocation shares. Expected base-unit amounts are independent integer values. Each synthetic account supplies `"vesting": {"kind": "unlocked"}` for this test only. Import and database reopen must retain every amount and the total.

The development importer supports only that explicit unlocked vesting object. It rejects a supplied locked schedule, unknown kind, extra schedule field, null, or malformed object before any database write. Missing vesting remains compatible with existing development fixtures. Missing vesting is not production approval. The production allocation review requires an explicit vesting input for each beneficiary.

The legacy core genesis template and its fixed-total validator retain historical raw units. The selected native importer rejects the historical full totals. An individual incorrectly labelled amount below the cap cannot reveal its original scale. No automatic legacy-state conversion exists. A migration requires a source format, an explicit source scale, recipient records, and a separate review.

Production beneficiaries and vesting terms must be explicit inputs. Do not use the historical core template to fill those records. The current native monetary importer does not enforce beneficiary custody or locked vesting schedules. It rejects supplied schedules that it cannot enforce. The unit contract does not close those implementation requirements.
