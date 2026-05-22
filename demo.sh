#!/bin/bash
# 🧠 MemoBuild Demo Script

echo "--- PHASE 1: INITIAL BUILD ---"
cargo run -- build

echo ""
echo "--- PHASE 2: RE-BUILD (SHOULD BE 100% CACHED) ---"
cargo run -- build

echo ""
echo "--- PHASE 3: CHANGE SOURCE CODE AND RE-BUILD ---"
echo "console.log('✨ MemoBuild detected a change!');" >> src/index.js
cargo run -- build

echo ""
echo "--- PHASE 4: CHECK CACHE STATUS ---"
cargo run -- info
