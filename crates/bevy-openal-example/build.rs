fn main() {
    openal_soft_build::ensure_openal_soft_binary()
        .expect("Failed to build OpenAL Soft binary for this target");
}
