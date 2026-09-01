#!/bin/sh
# Cold-start benchmark: builds a binary that exits right after its first
# frame is presented, then measures full process wall time with hyperfine.
# M1 gate: median < 100ms on a ~50KB file. Stretch: < 50ms.
set -e
cd "$(dirname "$0")/.."

FILE="${1:-samples/showcase.md}"

cargo build --release --features bench-first-frame
cp target/release/skimd target/release/skimd-bench
cargo build --release

hyperfine --warmup 3 "./target/release/skimd-bench $FILE"
