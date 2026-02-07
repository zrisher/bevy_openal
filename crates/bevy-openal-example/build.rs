use bevy_openal::build_support;

fn main() {
    build_support::ensure_openal_soft_binary()
        .expect("Failed to build OpenAL Soft binary for this target");
}
