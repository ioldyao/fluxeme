# Gates: handlers and database module split

OWNS: src/db/**, src/server/handlers/**, src/server/handlers.rs, src/server/mod.rs, GATES.md

Scope: Split the two oversized Rust implementation files into domain modules without changing public route symbols, database contracts, or runtime behavior.

- [x] G1: The Rust workspace formats successfully after the split
  CHECK: cargo fmt --all -- --check && printf 'FORMAT_OK\n'
  EXPECT: FORMAT_OK
  EVIDENCE: exit=0; shell=/bin/sh; cwd=/home/ezell/aigc/fluxeme; path=e959b33082d8/57 entries; output=FORMAT_OK

- [x] G2: The application compiles after the split
  CHECK: cargo check && printf 'CHECK_OK\n'
  EXPECT: CHECK_OK
  EVIDENCE: exit=0; shell=/bin/sh; cwd=/home/ezell/aigc/fluxeme; path=e959b33082d8/57 entries; output=warning: `fluxeme` (bin "fluxeme") generated 14 warnings | Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.79s

- [x] G3: Existing Rust tests pass after the split
  CHECK: cargo test && printf 'TEST_OK\n'
  EXPECT: TEST_OK
  EVIDENCE: exit=0; shell=/bin/sh; cwd=/home/ezell/aigc/fluxeme; path=e959b33082d8/57 entries; output=Finished `test` profile [unoptimized + debuginfo] target(s) in 1.14s | Running unittests src/main.rs (target/debug/deps/fluxeme-d137d284e21d67ed)

- [x] G4: The expected module tree exists and oversized legacy files are removed
  CHECK: node .unlazy/verify-structure.mjs
  EXPECT: STRUCTURE_OK
  EVIDENCE: exit=0; shell=/bin/sh; cwd=/home/ezell/aigc/fluxeme; path=e959b33082d8/57 entries; output=STRUCTURE_OK

- [x] G5: Key application flows remain present in the code graph
  CHECK: node .unlazy/verify-graph.mjs
  EXPECT: GRAPH_OK
  EVIDENCE: exit=0; shell=/bin/sh; cwd=/home/ezell/aigc/fluxeme; path=e959b33082d8/57 entries; output=GRAPH_OK
