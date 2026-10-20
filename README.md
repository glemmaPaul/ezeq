# Ezeq

Ezeq is a Rust scaffold for a modern parametric EQ plugin. It provides eight
smoothed biquad bands and a resizable spectrum-analyzer editor.

## Formats and framework

- VST3 for the requested DAW workflow
- CLAP from the same binary, for hosts that support it
- [NIH-plug](https://github.com/robbert-vdh/nih-plug) for host integration
- `egui` for the initial editor shell

NIH-plug is pinned to a commit so builds stay reproducible. The Rust toolchain
is also pinned so the resolved dependency graph builds consistently.

## Build

Install [Rust via rustup](https://rustup.rs/), then run:

```sh
cargo xtask bundle ezeq --release
```

The bundles are written to:

```text
target/bundled/Ezeq.vst3
target/bundled/Ezeq.clap
```

On macOS, install them for the current user:

```sh
mkdir -p ~/Library/Audio/Plug-Ins/VST3 ~/Library/Audio/Plug-Ins/CLAP
cp -R target/bundled/Ezeq.vst3 ~/Library/Audio/Plug-Ins/VST3/
cp -R target/bundled/Ezeq.clap ~/Library/Audio/Plug-Ins/CLAP/
```

Rescan plugins in the DAW after installation. During development, a symlink
from those plugin directories to `target/bundled` is faster than copying after
every build.

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask bundle ezeq
```

For quick UI and audio testing without a DAW, run the standalone build:

```sh
cargo run -p ezeq --release
```

Use `-- --help` to see CoreAudio input and output device options.

Run debug bundles while developing: NIH-plug's `assert_process_allocs` feature
will abort processing if the audio callback allocates. Test release bundles
with a plugin validator and in multiple hosts before distributing them.

## Layout

```text
crates/ezeq/src/lib.rs      Host formats, channel layouts, lifecycle
crates/ezeq/src/params.rs   Stable host parameters and persisted state
crates/ezeq/src/editor.rs   Editor composition and drawing
crates/ezeq/src/dsp/        Real-time filters and spectrum analysis
xtask/                      Cross-platform plugin bundling command
```

The audio callback must remain bounded and non-blocking: no heap allocation,
locks, logging, file/network I/O, or unbounded work. Allocate buffers in
`initialize`, communicate with the editor through atomics or lock-free queues,
and smooth automated parameters before applying them to filters.

## Suggested implementation order

1. [x] Add a tested biquad and coefficient generator.
2. [x] Model EQ bands with permanent, stable parameter IDs.
3. [x] Smooth gain, frequency, and Q changes without zipper noise.
4. [x] Add click-free band and global bypass.
5. Feed an FFT analyzer through a bounded lock-free channel.
6. Draw and edit the response curve in the editor.
7. Add oversampling only where measurements show it is needed.
8. Validate state migration, automation, mono/stereo operation, and unusual
   host block sizes.

The VST3 class ID and published parameter IDs are session compatibility
contracts. Do not change them after releasing the plugin.
