# Decision 0213: Separate native encryption-owner accounting

Date: 2026-09-20

Status: Accepted implementation; complete native regression and strict lint passed.
Excluded from Decision 0211's earlier binary.

Extend Decisions 0203/0204's six fixed native owner slots with Decision 0212's separate
encryption report, sampled at exactly the same successful-command handoff/drop boundaries.
Admission of an owner is atomic across both reports and all checked totals. Duplicate slots
and any overflow refuse without partially recording an owner. Consuming recovery/coordinator
handoffs remain one vault lifetime; no additional snapshots, I/O or authorization facades.

Preserve the existing decrypt-only `owner_vault_work`, `terminal_vault_work`, query deltas
and sampled decrypt measurements unchanged. Add a sibling `owner_vault_encryption_work`
to native BM-01 setup and BM-06 phase reports. Its owner labels and command scope match the
decrypt ledger; its fields are completed successful/failed encrypt calls, successful encoded
output bytes and unpadded input bytes. A successful encrypt is not a write, durable byte,
nonce allocation count or permission to sum ciphertext and adapter traffic as device I/O.

Both ledgers remain explicitly partial. The old decrypt ledger truthfully continues to
exclude encryption; the new encryption ledger excludes key wrapping, failed-call byte work
and owners discarded by failed commands. Complete authenticated I/O remains false. Neither
the qualifying profile caps nor frozen targets, reservations and sample counts change.

Verify checked atomic accounting and independent field totals, all native phases and process
paths, exact owner correspondence, positive construction encryption and unchanged encryption
across read-only domain admission. Preserve golden/nonce/fault tests and prior artifacts.
No consumer-interface or M1 pinned-handoff change is implied.

Initial release-test compilation found three existing history-owner test calls with the old
signature. Updated them for paired reports and added encryption-report failure atomicity checks
for both ledgers. The original attempt ran no tests; its exit 101 and log remain in PROGRESS.
The next run passed 85 library tests but failed six new BM-01 assertions (two existing ignores):
the draft incorrectly assumed every terminal owner encrypts zero bytes. Storage's existing
`finish_disk_blob_recovery` stages/publishes missing or stale derived catalogs. For these
inventory-free BM-01 creations the terminal cold open encrypts one META run page and one root
envelope. BM-06's fresh terminal after history validation normally finds that catalog current.

Capture `terminal_storage_open_encryption_work` before BM-01 domain admission/binding and
require exact equality with the terminal owner's later report, including process-loss resume.
Pin two successful terminal encryptions for creation and zero encryption/writes on the following
already-current open. Do not subtract legitimate rebuild work or relabel it as durable I/O.
This corrects a new test assumption, not engine behavior or existing acceptance requirements.
All seven focused BM-01 tests subsequently passed. The final complete native run passed
123 active tests, with five unchanged opt-in ignores, and strict Clippy. Scope peak was
547,528,704 bytes/zero swap; exact commands, failed attempts and per-suite timings remain in
PROGRESS. No core source changed after Decision 0212's 736-test all-feature workspace gate.
The frozen benchmark targets, missing qualification and M1 pinned handoff remain unchanged.
