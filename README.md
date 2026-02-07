# bevy_openal

Bevy plugin + runtime for [OpenAL Soft](https://github.com/kcat/openal-soft), with a CLI example in `crates/bevy-openal-example`.

> [!WARNING]
> I was having a lot of trouble getting this to work across a variety of platforms due to the Rust/C bridge.
> I’m abandoning this approach for AAA audio in Bevy in favor of [bevy_seedling](https://github.com/CorvusPrudens/bevy_seedling), which is the Bevy Audio Working Group’s current “v2” audio solution, along with avian3d for occlusion and reverb detection.

See the crate-specific docs:

- [bevy-openal](crates/bevy-openal/README.md)
- [bevy-openal-example](crates/bevy-openal-example/README.md)
