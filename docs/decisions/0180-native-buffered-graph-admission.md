# Decision 0180: Native buffered graph admission

Date: 2026-09-19

Status: Accepted

Select Decision 0179's opt-in cold graph admission in the packed development engine shared by
model/native BM-01 and BM-06. Use the existing 64 MiB logical cache size, sequentially per
canonical family and then for a fresh semantic phase; no cache is retained in the resulting
coordinator or used to prewarm query measurements. Exact partial-prefix cardinality, owner,
source identity, retry, v1 digest and terminal/pending boundaries remain unchanged.

Native reports identify the graph-admission budget and cache scope, and explicitly disclose
that coordinator primary/quota admission is not buffered. Existing vault totals expose actual
last-owner completed decrypt work without implying full multi-owner accounting or physical I/O.
All native create/open/resume/rebuild/query/sampling and BM-06 history/process-loss tests remain
required. Qualifying dimensions, deadlines, fixture counts, typed outcomes, oracle digest domains
and all performance targets are unchanged. The pre-optimization Decision 0177 measurements stay
pinned and preserved; a later measurement must name its own exact binary/source version.
