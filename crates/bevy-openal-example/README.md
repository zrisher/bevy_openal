# Bevy OpenAL Example

This crate is a headless CLI that demonstrates `bevy-openal`.
It builds [OpenAL Soft](https://github.com/kcat/openal-soft) during `cargo build`,
then launches a REPL where you can generate or load samples and play positional
one-shots while switching OpenAL output modes.

## Run

From the workspace root:

```
cargo run -p bevy-openal-example
```

You should see a prompt and a preloaded `beep` sample. Type `help` to list commands.

## [OpenAL Soft](https://github.com/kcat/openal-soft) Build Notes

The build script compiles [OpenAL Soft](https://github.com/kcat/openal-soft) from source and copies the platform library next to the
example executable. It requires CMake and a working C/C++ toolchain for your platform.

Environment variables:

- `OPENAL_SOFT_SOURCE_DIR` to use an existing source checkout
- `OPENAL_SOFT_REF` to choose a tag (default: `1.23.1`)
- `OPENAL_SOFT_URL` to override the download URL
- `OPENAL_SOFT_FORCE_REBUILD=1` to force a rebuild
