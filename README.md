<p align="center">
  <img src="assets/icon/AppIcon-macos.svg" width="140" alt="skimd icon">
</p>

# skimd

A markdown viewer for macOS optimised for speed of opening and beautiful rendering.

Editing is handelled on click, with blocks edited inline. (Don't forget to save!)

## Build

```sh
cargo build --release
./target/release/skimd samples/showcase.md
```

`./scripts/mac-bundle.sh` builds `target/Skimd.app`, which you can register for double-clicking markdown files in Finder.

## Test

```sh
cargo test
```
