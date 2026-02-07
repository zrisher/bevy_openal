use anyhow::{bail, Context, Result};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_OPENAL_REF: &str = "1.23.1";

/// Ensures the OpenAL Soft shared library is present in the current target directory.
///
/// This is designed to be called from a consumer `build.rs`. It will compile OpenAL Soft via
/// CMake (downloading sources by default) and copy the resulting shared library next to the
/// final build artifacts (`target/{profile}/`).
///
/// Environment variables:
/// - `OPENAL_SOFT_SOURCE_DIR`: use an existing source checkout instead of downloading
/// - `OPENAL_SOFT_REF`: tag to download (default: `1.23.1`)
/// - `OPENAL_SOFT_URL`: override download URL (default points at the GitHub tag zip)
/// - `OPENAL_SOFT_FORCE_REBUILD=1`: force rebuild even if the output already exists
///
/// Returns the output path of the copied shared library.
pub fn ensure_openal_soft_binary() -> Result<PathBuf> {
    println!("cargo:rerun-if-env-changed=OPENAL_SOFT_FORCE_REBUILD");
    println!("cargo:rerun-if-env-changed=OPENAL_SOFT_SOURCE_DIR");
    println!("cargo:rerun-if-env-changed=OPENAL_SOFT_REF");
    println!("cargo:rerun-if-env-changed=OPENAL_SOFT_URL");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").context("OUT_DIR not set")?);
    let target_dir = derive_target_dir(&out_dir)?;
    let target_os = std::env::var("CARGO_CFG_TARGET_OS")
        .context("CARGO_CFG_TARGET_OS not set")?
        .to_lowercase();
    let output_name = output_name_for_target(&target_os)?;
    let output_path = target_dir.join(output_name);

    if output_path.exists() && std::env::var("OPENAL_SOFT_FORCE_REBUILD").is_err() {
        return Ok(output_path);
    }

    let cache_root = target_dir
        .parent()
        .context("Failed to derive target root")?
        .join("openal-soft");
    let source_dir = resolve_source_dir(&cache_root)?;
    let profile_dir_name = target_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("debug");
    let build_dir = cache_root.join("build").join(profile_dir_name);

    fs::create_dir_all(&build_dir).context("Failed to create OpenAL build directory")?;

    let build_profile = cmake_profile();
    configure_openal(&source_dir, &build_dir, &build_profile)?;
    build_openal(&build_dir, &build_profile)?;

    let built_library = locate_built_library(&build_dir, &target_os)?;
    fs::copy(&built_library, &output_path).with_context(|| {
        format!(
            "Failed to copy OpenAL library from {} to {}",
            built_library.display(),
            output_path.display()
        )
    })?;

    Ok(output_path)
}

fn derive_target_dir(out_dir: &Path) -> Result<PathBuf> {
    let mut cursor = out_dir;
    while let Some(parent) = cursor.parent() {
        if cursor
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name == "build")
            .unwrap_or(false)
        {
            return Ok(parent.to_path_buf());
        }
        cursor = parent;
    }
    bail!("Failed to derive target directory from OUT_DIR");
}

fn output_name_for_target(target_os: &str) -> Result<&'static str> {
    match target_os {
        "windows" => Ok("OpenAL32.dll"),
        "linux" => Ok("libopenal.so.1"),
        "macos" => Ok("libopenal.dylib"),
        other => bail!("Unsupported target OS: {other}"),
    }
}

fn resolve_source_dir(cache_root: &Path) -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("OPENAL_SOFT_SOURCE_DIR") {
        let source = PathBuf::from(dir);
        if !source.exists() {
            bail!(
                "OPENAL_SOFT_SOURCE_DIR does not exist: {}",
                source.display()
            );
        }
        return Ok(source);
    }

    let reference = std::env::var("OPENAL_SOFT_REF").unwrap_or_else(|_| DEFAULT_OPENAL_REF.into());
    let source_root = cache_root.join("src");
    let expected = source_root.join(format!("openal-soft-{reference}"));

    if expected.exists() {
        return Ok(expected);
    }

    #[cfg(not(feature = "download"))]
    bail!(
        "OPENAL_SOFT_SOURCE_DIR is not set and the `download` feature is disabled (cannot fetch sources)"
    );

    #[cfg(feature = "download")]
    {
        fs::create_dir_all(&source_root).context("Failed to create OpenAL source directory")?;

        let url = std::env::var("OPENAL_SOFT_URL").unwrap_or_else(|_| {
            format!("https://github.com/kcat/openal-soft/archive/refs/tags/{reference}.zip")
        });
        let archive_path = source_root.join(format!("openal-soft-{reference}.zip"));

        if !archive_path.exists() {
            download_zip(&url, &archive_path)?;
        }

        extract_zip(&archive_path, &source_root)?;

        if expected.exists() {
            return Ok(expected);
        }

        find_first_openal_dir(&source_root).context("Unable to locate extracted OpenAL source")
    }
}

#[cfg(feature = "download")]
fn download_zip(url: &str, dest: &Path) -> Result<()> {
    let response =
        reqwest::blocking::get(url).with_context(|| format!("Download failed: {url}"))?;
    let mut response = response.error_for_status()?;
    let mut file =
        File::create(dest).with_context(|| format!("Failed to create {}", dest.display()))?;
    std::io::copy(&mut response, &mut file)?;
    Ok(())
}

#[cfg(feature = "download")]
fn extract_zip(archive_path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(archive_path)
        .with_context(|| format!("Failed to open {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("Failed to read {}", archive_path.display()))?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let Some(entry_path) = file.enclosed_name() else {
            continue;
        };
        let out_path = dest.join(entry_path);
        if file.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out_file = File::create(&out_path)?;
        std::io::copy(&mut file, &mut out_file)?;
    }
    Ok(())
}

#[cfg(feature = "download")]
fn find_first_openal_dir(root: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("openal-soft-"))
                .unwrap_or(false)
        {
            return Some(path);
        }
    }
    None
}

fn cmake_profile() -> String {
    match std::env::var("PROFILE")
        .unwrap_or_else(|_| "debug".into())
        .as_str()
    {
        "release" => "Release".to_string(),
        _ => "Debug".to_string(),
    }
}

fn configure_openal(source_dir: &Path, build_dir: &Path, profile: &str) -> Result<()> {
    let status = Command::new("cmake")
        .arg("-S")
        .arg(source_dir)
        .arg("-B")
        .arg(build_dir)
        .arg("-DCMAKE_POLICY_VERSION_MINIMUM=3.5")
        .arg("-DALSOFT_EXAMPLES=OFF")
        .arg("-DALSOFT_UTILS=OFF")
        .arg("-DALSOFT_TESTS=OFF")
        .arg("-DALSOFT_STATIC=OFF")
        .arg(format!("-DCMAKE_BUILD_TYPE={profile}"))
        .status()
        .context("Failed to run cmake")?;

    if !status.success() {
        bail!("CMake configure failed");
    }
    Ok(())
}

fn build_openal(build_dir: &Path, profile: &str) -> Result<()> {
    let status = Command::new("cmake")
        .arg("--build")
        .arg(build_dir)
        .arg("--config")
        .arg(profile)
        .arg("--parallel")
        .status()
        .context("Failed to run cmake build")?;

    if !status.success() {
        bail!("CMake build failed");
    }
    Ok(())
}

fn locate_built_library(build_dir: &Path, target_os: &str) -> Result<PathBuf> {
    match target_os {
        "windows" => find_named_file(build_dir, "OpenAL32.dll"),
        "macos" => find_named_file(build_dir, "libopenal.dylib"),
        "linux" => {
            if let Ok(path) = find_named_file(build_dir, "libopenal.so.1") {
                return Ok(path);
            }
            find_prefix_file(build_dir, "libopenal.so")
        }
        other => bail!("Unsupported target OS: {other}"),
    }
}

fn find_named_file(root: &Path, file_name: &str) -> Result<PathBuf> {
    if let Some(path) = find_file_recursive(root, |path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| name == file_name)
            .unwrap_or(false)
    }) {
        return Ok(path);
    }
    bail!("Failed to locate {file_name} in {}", root.display());
}

fn find_prefix_file(root: &Path, prefix: &str) -> Result<PathBuf> {
    if let Some(path) = find_file_recursive(root, |path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with(prefix))
            .unwrap_or(false)
    }) {
        return Ok(path);
    }
    bail!("Failed to locate {prefix}* in {}", root.display());
}

fn find_file_recursive<F>(root: &Path, predicate: F) -> Option<PathBuf>
where
    F: Fn(&Path) -> bool + Copy,
{
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file_recursive(&path, predicate) {
                return Some(found);
            }
        } else if predicate(&path) {
            return Some(path);
        }
    }
    None
}
