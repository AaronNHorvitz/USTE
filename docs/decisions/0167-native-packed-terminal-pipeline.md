# Decision 0167 — Native packed terminal pipeline

Date: 2026-09-19

Status: implemented and locally verified; no benchmark qualification.

Add separate Linux/Btrfs packed create, terminal open, explicit terminal rebuild and correctness
query phases, using a distinct `bm01-linux-packed-engine` database. Preserve all existing v1
commands/stores and the native development cap. Use the existing descriptor-based credential
checks, OS entropy, portable recovery and real clock; never test entropy or memory-model storage.
Share the packed fixture limits, authorized write/rebase and cold admission code with Decision 0166.

Create never replaces an existing database. Only policy bootstrap uses the ordinary reducer.
Cold open requires an independently admitted terminal triple and never silently reconstructs it.
Explicit rebuild authenticates the source fixture binding before creating replacement derived
roots, reconstructs to the actual frontier with zero recovery overlays, and does not append or
truncate authoritative transactions. These initial terminal commands require the expected complete
frontier; incomplete materializations remain retained for subsequent explicit prefix-resume work.
Do not pretend a terminal-only interface is a process-loss/resume qualification campaign.

Queries consume the separately generated bounded oracle summary, not an in-process adjacency
oracle. Keep exact output/typed-refusal checks, current authorization and bounded cache controls.
Reports remain content-free, nonqualifying and explicit about uncontrolled host caches, partial
adapter accounting, lack of preemptive query deadlines and outstanding repeated cold/warm sampling.
Test real close/open, retries/rebuild authority preservation, wrong credentials/profiles, absent
or corrupt derived roots and committed-source corruption. Native prefix recovery/process-loss,
packed BM-06 history, complete authenticated work accounting and qualification remain open.

Verification: four native library cases and two separate-process CLI cases pass, including all
384 independently supplied oracle queries and every fixture-batch retry. Full experiment regression
passes 80 active tests with two pre-existing ignored campaigns; strict Clippy passes. See PROGRESS
for commands, resource limits and corrected test-harness failures. Actual native packed fixtures
use 20 entities/200 relationships; the 20,000-entity admission ceiling is not a measured result.
