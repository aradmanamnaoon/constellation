# Constellation — platform acceptance ledger

The authoritative, editable **188-row acceptance inventory** is maintained in
[`docs/quality/acceptance-ledger.md`](../quality/acceptance-ledger.md).

This entrypoint exists because the original specification calls for the ledger
under `docs/qa/`. To avoid two divergent sets of PASS/FAIL states, there is
only **one** ledger: the linked file above. It begins with `UNVERIFIED` rows;
individual tests, source inspections, mocks, uninstalled GUI binaries and
unsigned packages do not automatically advance an entire platform requirement.

For specific source-pinned evidence, see
[`docs/qa/`](./) and the
[`23-workstream execution plan`](../mission/COMPLETE_EXECUTION_PLAN.md).
