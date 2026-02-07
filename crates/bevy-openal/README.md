# Bevy OpenAL ([OpenAL Soft](https://github.com/kcat/openal-soft))

This crate provides a Bevy plugin plus a lightweight [OpenAL Soft](https://github.com/kcat/openal-soft) runtime. The runtime owns device
I/O, basic 3D rendering, and realtime-safe playback, while the Bevy integration is intentionally
thin and explicit.

## Features (Current)

- [OpenAL Soft](https://github.com/kcat/openal-soft) backend loaded dynamically at runtime.
- Output render modes: `Auto`, `Stereo (Clean)`, `Headphones (HRTF)`, `Surround (Auto)`.
- Runtime thread with a small command surface:
  - set render mode / mute
  - update listener frame
  - register mono PCM buffers
  - play simple one-shots
- Status snapshot for HUD/logs (`AudioRuntimeStatus`).
- Decode helper (`decode_to_mono_i16`) that downmixes to mono 16-bit PCM.

## Not In This Crate (By Design)

- Cue/event system, layering, switching, or RTPC-style parameters.
- Voice management, concurrency, buses, snapshots, ducking, or mastering.
- Streaming, music playback, or long-form ambience.
- Occlusion, reverb, propagation, or geometry-based effects.
- Bevy asset loaders, console commands, or debug HUD.

These live in the client and sit on top of this runtime.

## Usage

1. Ship the [OpenAL Soft](https://github.com/kcat/openal-soft) shared library next to the client executable.
2. Create an `AudioRuntime` with your preferred render mode.
3. Decode audio bytes to mono PCM and register a buffer key.
4. Update the listener every frame and issue one-shot plays as needed.

```rust
use bevy_openal::{
    decode_to_mono_i16, AudioRenderMode, AudioRuntime, AudioRuntimeConfig, BufferKey,
    ListenerFrame, PlayOneShotParams,
};

let runtime = AudioRuntime::new(AudioRuntimeConfig {
    initial_render_mode: AudioRenderMode::Auto,
    ..Default::default()
})?;

let decoded = decode_to_mono_i16(&bytes)?;
let key: BufferKey = 1;
runtime.create_buffer(key, decoded)?;

runtime.set_listener(ListenerFrame {
    position,
    forward,
    up,
    velocity,
})?;

runtime.play_one_shot(
    key,
    PlayOneShotParams {
        position,
        ..Default::default()
    },
)?;
```

## Bevy Integration (Quick Start)

```rust
use bevy::prelude::*;
use bevy_ecs::message::MessageWriter;
use bevy_openal::{BevyOpenalPlugin, OpenalListener, OpenalPlayOneShot};

App::new()
    .add_plugins(MinimalPlugins)
    .add_plugins(TransformPlugin)
    .add_plugins(BevyOpenalPlugin)
    .add_systems(Startup, |mut commands: Commands| {
        commands.spawn((OpenalListener, Transform::default(), GlobalTransform::default()));
    })
    .add_systems(Update, |mut writer: MessageWriter<OpenalPlayOneShot>| {
        writer.write(OpenalPlayOneShot {
            key: 1,
            position: Vec3::new(0.0, 0.0, -2.0),
            gain: 1.0,
            pitch: 1.0,
        });
    })
    .run();
```

Notes:

- The runtime currently expects **mono** 16-bit PCM buffers (`AL_FORMAT_MONO16`).
- `AudioRuntimeStatus` is available via `runtime.status()` for UI/telemetry.
- Device switching is not implemented yet; you can only choose a preferred device at startup.

## Packaging ([OpenAL Soft](https://github.com/kcat/openal-soft))

The loader searches the executable directory first, then falls back to the platform search path.

- Windows: `OpenAL32.dll` next to `client.exe`
- Linux: `libopenal.so.1` next to the binary
- macOS: `libopenal.dylib` next to the binary inside the `.app` (typically `Contents/MacOS/`)

The client build script can build [OpenAL Soft](https://github.com/kcat/openal-soft) from source and copy the library into the target
directory. This requires CMake and a C/C++ toolchain for the current platform. Environment
variables for the build step:

- `OPENAL_SOFT_SOURCE_DIR` to use an existing source checkout
- `OPENAL_SOFT_REF` to choose a tag (default: `1.23.1`)
- `OPENAL_SOFT_URL` to override the download URL
- `OPENAL_SOFT_FORCE_REBUILD=1` to force a rebuild

## Render Modes Summary

- `Auto`: Let OpenAL choose the best output mode for the device.
- `Stereo (Clean)`: No in-game HRTF; useful with OS/headset virtualization.
- `Headphones (HRTF)`: In-game binauralization; avoid double-processing.
- `Surround (Auto)`: Discrete multichannel output (e.g., 5.1/7.1).

---

# Design Goals & Roadmap

This crate is the foundation of the ZRG Shooter audio runtime. It already provides the OpenAL
device/context, basic 3D playback, and a minimal command surface. The sections below capture the
current decisions, design targets, and future work so this crate can evolve without needing
external planning docs.

Current status (as of 2026-02-06):

- Backend decision is locked: [OpenAL Soft](https://github.com/kcat/openal-soft) (dynamic library)
- Player-facing render modes are locked (Auto / Stereo Clean / Headphones HRTF / Surround Auto)
- Near-term switching is via dev console commands (settings UI later)

## Design Targets

These are the targets we are building toward. Some are already implemented in this crate, while
others are planned in the surrounding client integration.

### Platform + Output

- Platforms: Windows, Linux, macOS
- Output render modes (player-facing):
  - `Auto`: choose best default per device (surround if >2 channels; otherwise stereo clean)
  - `Stereo (Clean)`: no in-game HRTF (best with OS/headset virtualization or external DSP)
  - `Headphones (HRTF)`: in-game binaural; warn about double-processing
  - `Surround (Auto)`: discrete multichannel bed (prefer 7.1 when available, else 5.1)
- Device selection + hot swap (USB headsets, HDMI, etc.)
  - Phase 1 scope is the default device only; device switching remains a TODO.

### Bevy Ecosystem Reality (2026)

- Bevy's built-in audio path is intentionally minimal and not designed for AAA spatial audio.
- Most Bevy projects that need better mixing/control adopt a third-party audio backend, but
  there is no single "standard" backend for AAA features like HRTF, surround, and propagation.
- For AAA scope, assume we will maintain our own audio runtime integration and treat Bevy as
  the gameplay/event producer.

### AAA Shooter Feature Expectations

- Data-driven audio "events" (cues) with:
  - variations (random, weighted)
  - layering (mechanical + blast + tail)
  - switches/selectors (surface type, indoor/outdoor, weapon attachments)
  - continuous parameters (RTPC-style curves)
  - sequencing and stingers (UI, music transitions)
- Robust voice management:
  - concurrency groups (gunshots, footsteps, UI, ambience)
  - priority + distance-based virtualization
  - voice stealing (quietest/oldest) with click-free fades
- Mixing:
  - buses (Master / SFX / UI / Music / VO / Ambience)
  - snapshots (underwater, low health, pause menu, inside vehicle)
  - sidechain ducking (UI/VO over SFX, explosions over ambience)
  - limiter and optional compression (protect ears / consistent loudness)
- 3D spatial rendering:
  - attenuation rolloff (tunable; gameplay-first, realism-informed)
  - air absorption (distance-based low-pass)
  - doppler (clamped, per-category)
  - propagation delay (optional "speed of sound" for distant shots)
  - occlusion/obstruction with smoothing (no zipper noise)
  - environmental reverb (zones and/or geometry-based)
- Streaming:
  - music and long ambience streams (no full decode in RAM)
  - seekable playback (menus, stingers)
- Tooling (non-negotiable for long term):
  - live audio debug HUD (voices, buses, levels, occlusion, priorities)
  - capture/replay of audio events for debugging
  - deterministic "audio test scenes" for regression checks

## Architecture (Backend-Agnostic)

Gameplay (ECS) should never talk directly to the mixer. It emits events; the audio runtime owns
threading, device I/O, decoding, mixing, and DSP.

## Crate Boundaries (Keep This Extractable)

This integration is expected to get gnarly (FFI + realtime threading + streaming + tooling). To keep
it maintainable long-term and make a future "split into its own repo" realistic:

- Keep as much implementation as possible in the runtime modules, with a thin Bevy adapter layer.
- Keep the API between the Bevy client and the audio crate intentionally small and stable.
  - Prefer a command-based API (`AudioCommand` -> audio thread) and a read-only status/debug snapshot.
- Any Bevy-specific glue (assets, ECS systems, UI debug overlay, console commands) should be thin and
  isolated (either behind an optional feature or in a separate crate/module).

Long-term public API target (current API is smaller):

- `AudioRuntime::start(AudioConfig) -> (AudioHandle, AudioStatusReceiver)`
- `AudioHandle::submit(AudioCommand)` (non-blocking; drops or coalesces spammy updates)
- `AudioStatusSnapshot` (read-only, periodic; for HUD + logs)
- Core commands (examples):
  - `SetRenderMode`, `SetMuted`
  - `SetListener { pos, forward, up, vel }`
  - `RegisterSample { id, bytes }` / `UnregisterSample`
  - `PlayCue { cue_id, emitter, params }` (cue resolution + voice mgmt happens in audio runtime)

Pipeline:

`Gameplay -> AudioEvents -> Cue Resolver -> Voice Manager -> Spatial Sim -> Renderer -> Buses -> Device`

Key concepts:

- `AudioCue`: data definition (RON/JSON) for one "event" (e.g., `weapon.m1.fire`)
- `AudioVoice`: a playing instance created from a cue layer
- `AudioBus`: mix group with volume + effect chain + snapshot support
- `AudioParam`: named runtime parameter (distance, occlusion, indoors, health, etc.)
- `AudioRenderMode`: Auto | StereoClean | HeadphonesHrtf | SurroundAuto

Threading model:

- Main thread: emits `AudioCommand`s, updates listener/emitter transforms, computes low-rate
  world queries (occlusion rays / zone membership), and sends results to audio thread.
- Audio thread: real-time safe; owns the OpenAL device/context, uploads/queues PCM buffers, updates
  sources/listener, applies EFX, and drives streaming. Heavy decode happens off-thread (decode worker).

## Configuration (Configurable, Not Hardcoded)

The current client config exposes render mode, source limits, preferred device, cue bank path, and
basic distance tuning. The full target config surface below captures the long-term tuning goals so
we can extend without rebuilding and support different platform/device expectations.

- `AudioConfig` (loaded at startup; path is configurable):
  - output: render mode (`Auto`/`StereoClean`/`HeadphonesHrtf`/`SurroundAuto`), sample rate preference (48k),
    device selection policy (phase 1: default device), debug HUD default
  - content: cue format + cue root directory
  - tuning: distance model defaults, voice budgets, occlusion defaults, doppler defaults, etc.

Cue format + location policy:

- Default cue format: RON
- Default cue root: `assets/audio/cues/`
- Configurable via `AudioConfig`:
  - `cue_format`: `ron` | `json` (future)
  - `cue_root`: path (relative to asset root)
- Hot reload:
  - Not required for now.
  - TODO: add a stub command + code path to reload cues in dev builds (e.g. `audio.reload_cues`),
    even if it is a no-op initially.

Asset conditioning guidelines (targets; configurable via pipeline tooling):

- Spatial SFX (3D): mono PCM `.wav`, 48 kHz, 16-bit
- Non-spatial SFX/UI: stereo PCM `.wav` is allowed when spatialization is not needed
- Music/ambience: streamable compressed format (e.g. `.ogg`), 48 kHz, stereo
- TODO: add a simple validator script + CI check to enforce sample rate/channel count conventions

## Recommended Tech Stack (No Paid Middleware)

### Primary plan (Chosen, AAA-capable core)

- Core backend: [OpenAL Soft](https://github.com/kcat/openal-soft) (device I/O + 3D renderer + HRTF + EFX)

What this buys us:

- A mature, cross-platform 3D audio backend that supports 5.1/7.1 output and HRTF, plus
  EFX environmental effects like reverb/occlusion/obstruction.

What this does not buy us:

- A full "middleware-like" bus graph, sidechain compressor, snapshot system, or geometry-based
  propagation/reflections. We still implement the event/cue layer, voice management, and "bus-like"
  controls (volumes/snapshots/ducking policies) in our own runtime.

OpenAL capability mapping (what lives where):

- [OpenAL Soft](https://github.com/kcat/openal-soft) provides: device I/O, per-voice 3D panning/attenuation, HRTF, multichannel output
- OpenAL EFX provides (when available): reverb and source filters (LPF/HPF) for occlusion/underwater/etc.
- Our runtime provides: cue/event system, parameter curves, voice mgmt/stealing/virtualization,
  "bus-like" volumes, snapshots, ducking via gain automation
- Mastering (limiter/compressor): not provided by OpenAL; initial approach is conservative headroom +
  content loudness discipline; revisit a software mastering stage later if needed

Licensing/packaging notes:

- [OpenAL Soft](https://github.com/kcat/openal-soft) is LGPL. Dynamic linking (shipping it as a shared library) is typically the lowest
  friction way to stay compliant, but static linking is not inherently impossible; it just adds
  obligations around relinking/user replacement. (Not legal advice.)
- Practical impact: this mainly affects how we ship and load the audio library; it does not
  materially affect audio runtime performance after startup.
- Packaging/search path rule (locked): ship the [OpenAL Soft](https://github.com/kcat/openal-soft) shared library alongside the executable.
  - Windows: `OpenAL32.dll` next to `client.exe`
  - Linux: `libopenal.so.1` next to the client binary
  - macOS: `libopenal.dylib` next to the binary inside the `.app` (typically `Contents/MacOS/`)
  - Runtime loader behavior: attempt to load from the executable directory first, then fall back to the
    platform dynamic loader search paths.

### Optional upgrade (Advanced propagation)

If we need higher-end geometry-based propagation/reflections/reverb later, we can evaluate adding
a separate propagation library. This is intentionally deferred until we have the OpenAL core shipped.

Note: Steam Audio is a candidate, but its upstream C API docs historically called out macOS
"64-bit Intel". Treat Apple Silicon support as unverified until we have a working arm64 build in CI.

### Alternative plan (Mixer-first)

- Core engine: `miniaudio` (device I/O, mixing, node graph, decoding/streaming)
- Spatialization: would still require an HRTF/propagation solution (not provided by `miniaudio`)

### Secondary plan (Rust-first, simpler, not the final AAA target)

- `kira` for mixing/buses/effects + custom spatial sim.
- HRTF would require: convolution implementation + a redistributable HRIR dataset.

This is viable for a strong stereo pipeline, but it is riskier for "AAA expected" HRTF/surround and
advanced propagation over the long term.

## Output Modes Strategy

- Default: `Auto`
- `Stereo (Clean)`:
  - Game outputs normal stereo (no binauralization / no positional virtualization)
  - If the user enables OS/driver "virtual surround", it will be a global post-process
    (the OS/driver does not receive per-sound directions from the game in this mode)
- `Headphones (HRTF)`:
  - Game performs binauralization per voice (positional stereo output)
  - UI hint: disable OS/headset virtualization to avoid double-processing artifacts
- `Surround (Auto)`:
  - Game outputs a discrete multichannel bed (no binaural)
  - OS/driver/headset virtualization can downmix multichannel to headphones because channel
    layout itself encodes directionality (no per-voice metadata needed)

Auto selection (initial rule; configurable):

- If device reports >2 channels, default to Surround; otherwise Stereo.
- Never try to detect "stereo post-DSP" (not reliably detectable); provide a manual toggle.

## Data Model (Draft)

- `CueId`: stable string or hashed identifier (e.g., `weapon.m1.fire`)
- `Cue`:
  - `layers`: list of `Layer` (each selects a sample, loop/one-shot, routing, params)
  - `variants`: weighted random selection for samples or layer sets
  - `switches`: pick variant by state (surface, indoors, armor, etc.)
  - `params`: named curves (distance->gain, occlusion->lp_cutoff, etc.)
  - `concurrency`: group + max voices + stealing rule
  - `priority`: base priority + param modifiers
- `Emitter` (ECS):
  - position, velocity (for doppler), category, occlusion settings, max distance
- `Listener` (ECS):
  - transform, velocity, render mode

## Roadmap Phases (Phase 0 largely complete)

### Phase 0: Decide and scaffold (completed)

- Backend: [OpenAL Soft](https://github.com/kcat/openal-soft) (chosen)
- Keep a thin Bevy adapter layer on top of the runtime modules
- Define `AudioCommand` API and a minimal `Cue` file format + `AudioConfig`

### Phase 1: Replace current weapon audio (next)

- Keep existing content (close/medium/far assets) but route through:
  - a cue system (`weapon.m1.fire`, `weapon.m1.reload.*`)
  - buses (SFX, UI)
  - concurrency limits and fades
- Listener/emitter transforms feed the audio runtime (no spawning Bevy `AudioPlayer` entities)
- Add a minimal debug overlay: active voices + bus meters

### Phase 2: Spatial sim + occlusion baseline (planned)

- Attenuation curves + air absorption (LPF) + clamped doppler
- Occlusion via raycasts (low rate) with smoothing and per-category tuning
- Add "speed of sound" delay (optional, weapon-only, distance-thresholded)

### Phase 3: Output modes (planned)

- Auto: stable, default (select Stereo Clean vs Surround Auto based on device channels)
- Stereo Clean: stable, always-available fallback
- Surround 5.1/7.1: validate channel layout and panning on each OS
- Headphones HRTF: enable/disable HRTF by reopening the OpenAL device/context
- Voice budget: cap spatial voices aggressively; degrade to non-spatial/quietest stealing

### Phase 4: Environments + reverb (planned)

- Zone-based reverb + EQ snapshots (fast, deterministic)
- Optional upgrade: geometry-informed propagation/reflections/reverb (deferred)
- Snapshot system for mixes (underwater, indoors, vehicle, pause)

### Phase 5: Tooling + regression (ongoing)

- Audio event capture/replay to reproduce mixes
- Deterministic audio test scenes for CI (render N frames and validate meters/voice counts)
- Runtime profiling (CPU %, voices, streaming stats)

## Risks / Open Questions

- Native dependencies: distribution, CI, platform quirks (especially macOS signing/notarization).
- LGPL compliance: ensure [OpenAL Soft](https://github.com/kcat/openal-soft) is dynamically linked and ship required notices. (Not legal advice.)
- Output mode switching: HRTF and channel layout changes may require device/context reopen.
- EFX availability: effect slot/filter limits and per-platform behavior need validation.
- macOS Apple Silicon support: ensure we ship an arm64 (or universal) [OpenAL Soft](https://github.com/kcat/openal-soft) build.
- Surround expectations: channel order/layout differs by APIs; needs careful mapping + tests.
- Multiplayer scale: far-away firefights can spam events; voice virtualization must be aggressive.
- Missing backend: if the [OpenAL Soft](https://github.com/kcat/openal-soft) library cannot be loaded/opened, the game must remain playable
  (run silent, surface a clear error in logs/HUD, and allow retry via mode switch/reinit).

## Locked Decisions

1) Output render mode UX: `Auto` / `Stereo (Clean)` / `Headphones (HRTF)` / `Surround (Auto)`
   - Switching is supported at runtime by reopening the OpenAL device/context.
   - Temporary UX: dev console command until a settings UI exists.

2) Device UX (phase 1): default device only
   - Add a TODO/stub for device switching later (no multi-device routing).

3) Rust integration: new crate with direct OpenAL dynamic loading
   - No `alto`; keep bindings minimal and explicit.

4) Mix/sample rate policy: target 48 kHz
   - Rationale: 48 kHz is the most common "system/video/game" rate; it reduces resampling on many devices.
   - Implementation: prefer requesting 48 kHz when opening the device/context; fall back to device default.

5) Asset formats: split by category
   - SFX: PCM `.wav` (fast start, predictable decode; good for one-shots)
   - Music/ambience: compressed streamable format (e.g. `.ogg`)

6) Decoding: pure-Rust decoding from Bevy-loaded bytes
   - Use `AudioSource`-style bytes as the ingestion format; decode on a dedicated decode worker (never block the audio thread).
   - 3D SFX are decoded/downmixed to mono PCM; non-spatial content (UI/music) may remain stereo.

7) Streaming: stream long-form content, preload latency-sensitive SFX
   - Music + long ambience: streaming
   - Weapon/UI/foley: preload (with an LRU for rare assets if needed)

8) Voice management: hard voice budgets + concurrency groups + virtualization
   - Aggressive per-category limits; distance/priority-based virtualization; click-free fades.

9) Spatial sim baseline: OpenAL 3D + game-authored tuning
   - Attenuation curves are gameplay-first; add optional air absorption (LPF) and clamped doppler.

10) Occlusion baseline: raycast + smoothing
   - Low-rate raycasts on the main thread; send smoothed occlusion values to audio thread.

11) Environments: zone-based reverb/snapshots first
   - Use OpenAL EFX where available; keep it deterministic and authorable.
   - Geometry-based propagation/reflections is explicitly deferred until core is shipped.

12) Debug tooling: in-game audio HUD, toggled via dev console
   - Minimum: active voices, budgets, device/mode, HRTF active, CPU-ish timing, streaming stats.

13) Initial tuning defaults live in config (conservative starter values)
   - Coordinate scale:
     - `meters_per_world_unit = 1.0` (treat 1 world unit as 1 meter; override if our world scale differs)
   - Distance model (OpenAL):
     - `distance_model = InverseDistanceClamped`
     - `reference_distance_m = 1.0`
     - `rolloff_factor = 1.0`
     - `max_distance_m = 80.0` (generic default; cues/categories override as needed)
   - Doppler:
     - `doppler_factor = 0.5` (subtle by default; enable per-category)
     - `speed_of_sound_mps = 343.0`
     - `doppler_pitch_clamp = [0.9, 1.1]` (avoid extreme pitch swings)
   - Air absorption (baseline LPF):
     - `air_absorption_start_m = 20.0`
     - `air_absorption_full_m = 120.0`
     - `air_absorption_min_hf = 0.25` (map to an EFX low-pass "HF gain" style control)
   - Occlusion (raycast + smoothing):
     - `max_occlusion_rays_per_frame = 64`
     - `occlusion_update_hz = 10`
     - `occlusion_attack_s = 0.10`
     - `occlusion_release_s = 0.25`
     - `occlusion_gain_db = -4.0`
     - `occlusion_lowpass_min_hf = 0.35`
   - Voice budgets (starter; tune in playtests):
     - `max_voices_total = 64`
     - category caps (examples): weapons 16, footsteps 12, impacts 16, ambience 8, ui 16, vo 8
   - "Speed of sound" delay (optional, weapon-only):
     - enable only beyond `distance_threshold_m = 50.0`
     - delay = distance / `speed_of_sound_mps` (clamped)

## Near-Term Work Items

- [OpenAL Soft](https://github.com/kcat/openal-soft) packaging for Win/Linux/mac (including macOS arm64/universal) still needs to be wired into
  installer/distribution (shared library alongside the executable).
- `AudioConfig` is loaded at startup with cue bank path + initial tuning defaults; extend it with the
  remaining tuning fields listed above as they are implemented.
- Cue bank loader uses RON at `assets/audio/cues/`, and a hot-reload command exists; continue expanding
  cue coverage across the game.
- Migrate remaining Bevy audio uses (weapons, footsteps, UI) to the cue-based runtime API.
