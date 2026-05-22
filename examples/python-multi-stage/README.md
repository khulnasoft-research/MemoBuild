# Python Multi-Stage Example

This example demonstrates how MemoBuild optimizes standard Docker multi-stage builds.

## MemoBuild Advantages

When you build this with MemoBuild:
- The graph executor parses the `builder` stage and the final runtime stage as separate but connected DAG structures.
- It detects explicitly what needs to be invalidated when files change. Modifying `main.py` will not invalidate the `pip install` layer!
- The dependencies (in `/opt/venv`) are preserved explicitly as OCI layers.

## Usage

Run the following command from this directory:

```bash
cargo run --bin memobuild -- --file Dockerfile --sandbox local
```
