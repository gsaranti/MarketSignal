# Fixed evidence set — big-run attempt 6

Reconstructed, not replayed.
Each `<symbol>.json` is reduced from the holding's persisted row of big-run attempt 6 (portfolio run `8d6b0b88-c00a-4b93-9bf0-13c7b1542eac`, `job_runs` id 6, `portfolio-v35`, launched 2026-09-16T02:04Z), extracted 2026-09-16 from the read-only store extracts.
The position's quantity, cost basis and market value are synthetic (ten units; a gain on four holdings, a loss on ARKF and PGNY).
The model-authored strings that referenced the account's position — every rationale and self-assessment, and DIA's per-share cost sentence — are replaced by labelled placeholders, and a test greps every fixture string for the economics phrases, so no account economics enter the repository.
`house-view.json` is the checkpoint header's house view, shared by every holding.
Each fixture also carries `equity_source`, the balance-sheet instants' source the run stamped: `fmp-quarterly` on the three stocks, whose computed debt / equity came off an FMP quarterly balance sheet, and null on the funds (fix list 8.5, ruled 2026-09-17).
With the field set, the prompt's balance-sheet line agrees with the computed metrics on the harness as it did on the run.

What each fixture carries: the vehicle kind and stamped statement basis, the authoring close and spot, every ledger condition as the model authored it with its expected 6g outcome (`kept`, `qualitative`, or `downgraded:<class>`), the ledger's prose, the persisted verdict disposition, the engine output assembled from the audit, and the distilled research text.
The ledger's prose (`ledger_prose`) is the persisted current thesis, the three monitor rows (scenario, conditions, probability) and the two must-lines, extracted from the same rows on 2026-09-17 so the action packet's THESIS and SCENARIOS sections render on the harness (`portfolio-v41`).
What it does not carry: the statement rows, the fund context, the option overlay and the pre-profit overlay, none of which persist.

Two synthetic cases live beside the reconstructed six in the same module (ruled 2026-09-16, C2): a role/risk fund ledger and a second-run continuity case over DIA's kept conditions, each labelled synthetic in its test; attempt 6 produced neither.
A third synthetic case, the role/risk fixture (`synthetic_role_risk_fixture`, ruled 2026-09-17, `portfolio-v42`), is a total bond market ETF at a hand-written spot with twenty-one hand-written closes, a hand-written research summary, a hand-written Treasury positioning line and the fixed set's house view; its readout is the fund engine's own on those inputs.
It renders the role/risk interpretation message (debut, and continuity on a stub first run) and the action packet's role/risk branch in the offline pins and the dump, and the live harness issues its interpretation and action calls after the six reconstructed holdings, printed under a SYNTHETIC label; it is evidence of the prompt's shape on a fund of that class, never of a run.
Consumers: `src/portfolio/fixed_evidence.rs` — the offline replay of the persisted drafts through 6g, the action-packet isolation checks, and the `#[ignore]`d live harness (`fixed_evidence_live`) that issues the interpretation and action calls against the local daemon on these packets.
With `MARKET_SIGNAL_LOCAL_EVAL_THOUGHT_DIR` set, the harness also captures each call's thinking, fenced per call, through the dev app's thought-log sink, so the per-call re-think read runs on the fixed set without a book-scale attempt.
The evaluation gate they serve is fix list §8.1 / §8.2 in `docs/verification/2026-09-15-local-model-call-fixes.md`; the slice record is `docs/verification/2026-09-16-ledger-conditions-and-action-packet.md`.
