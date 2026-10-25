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

`nih_plug_egui` is taken from the
[egui-baseview](https://github.com/BillyDM/egui-baseview) repository instead of
NIH-plug. That version only captures keyboard input when egui needs it (for
example a focused text field) and otherwise passes key events on to the host,
so DAW shortcuts and Ableton's computer MIDI keyboard keep working while the
editor is open. Its pinned NIH-plug commit must match the one in `Cargo.toml`.

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

On Windows, run the same `cargo xtask bundle ezeq --release` with the MSVC
toolchain (install the Visual Studio Build Tools with "Desktop development with
C++" first) and copy the bundles to:

```text
C:\Program Files\Common Files\VST3\Ezeq.vst3
C:\Program Files\Common Files\CLAP\Ezeq.clap
```

Rescan plugins in the DAW after installation. During development, a symlink
from those plugin directories to `target/bundled` is faster than copying after
every build.

## Releases

CI builds Windows (x86_64) and macOS (universal) bundles on every push and
uploads them as workflow artifacts. Pushing a `v*` tag additionally publishes a
GitHub release with the zipped bundles attached:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The macOS bundles are ad-hoc signed but not notarized, so Gatekeeper will warn
on first launch until the plugin is signed with a Developer ID.

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
5. [x] Feed an FFT analyzer through a bounded lock-free channel.
6. [x] Draw and edit the response curve in the editor.
7. [x] Measure before adding oversampling. High-frequency THD+N stays below
   -90 dB for every filter type at 44.1, 48, and 96 kHz, so the current linear
   signal path does not use oversampling.
8. [x] Validate state migration, smoothed automation, mono/stereo channel
   isolation, and host blocks from empty and single-sample buffers through
   blocks larger than the configured maximum.

## Dynamic EQ and sidechain roadmap

Each plugin instance will expose one mono/stereo external sidechain bus. All
bands may use that shared signal, with an independent frequency-selective
detector for each band. Projects that need different sidechain sources for
different bands should use multiple Ezeq instances.

NIH-plug presents an unconnected sidechain as silence. External sidechain
routing therefore needs to be tested in a VST3/CLAP host; the CoreAudio/CPAL
standalone build cannot receive a separate sidechain signal (the JACK backend
can).

1. [x] Declare the sidechain bus in the mono and stereo audio layouts and pass
   the host-provided auxiliary buffer into the DSP engine.
2. [x] Add stable per-band parameters for dynamic range, threshold,
   attack/release, and internal or external trigger selection.
3. [x] Implement a band-limited detector that follows each band's frequency and
   Q, with a linked and predictable response for mono and stereo sidechains.
4. [x] Add a real-time-safe envelope follower and compression/expansion curve
   that modulates band gain without clicks or per-block allocations.
5. [x] Keep the sidechain signal detector-only: it must never be mixed into the
   main output, and missing sidechain input must fail safely.
6. [x] Show per-band dynamic range and live gain reduction in the editor, with
   sidechain audition or spectrum visualization as a later enhancement.
7. [x] Test internal and external triggering, silence and disconnected
   sidechains, automation, mono/stereo layouts, unusual block sizes, reset
   behavior, and state migration.
8. [ ] Validate sidechain routing and restored sessions in representative VST3
   and CLAP hosts.

The VST3 class ID and published parameter IDs are session compatibility
contracts. Do not change them after releasing the plugin.
