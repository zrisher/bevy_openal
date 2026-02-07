use crate::sound;
use bevy_app::{ctrlc, AppExit};
use bevy_ecs::message::MessageWriter;
use bevy_ecs::prelude::{NonSendMut, ResMut};
use bevy_math::Vec3;
use bevy_openal::{
    decode_to_mono_i16, AudioRenderMode, DistanceModel, OpenalRuntime, PlayOneShotParams,
};
use shell_words::split;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

static CTRL_C_REQUESTED: AtomicBool = AtomicBool::new(false);
static PROMPT_SHOWN: AtomicBool = AtomicBool::new(false);
static PROMPT_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let make_writer = || PromptingWriter::new();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(make_writer)
        .init();
}

pub(crate) fn start_command_receiver() -> CommandReceiver {
    let (tx, rx) = mpsc::channel::<String>();
    let shutdown = Arc::new(AtomicBool::new(false));
    install_ctrlc_handler();
    let shutdown_for_thread = Arc::clone(&shutdown);
    std::thread::spawn(move || read_stdin(tx, shutdown_for_thread));

    CommandReceiver { rx, shutdown }
}

pub(crate) fn handle_ctrlc_exit(
    mut receiver: NonSendMut<CommandReceiver>,
    mut runtime: Option<ResMut<OpenalRuntime>>,
    mut exit: MessageWriter<AppExit>,
) {
    if CTRL_C_REQUESTED.swap(false, Ordering::Relaxed) {
        if let Some(runtime) = runtime.as_deref_mut() {
            runtime.shutdown();
        }
        receiver.request_shutdown();
        exit.write(AppExit::from_code(130));
    }
}

pub(crate) fn ensure_prompt_visible() {
    print_prompt_once();
}

pub(crate) fn print_prompt() {
    PROMPT_SHOWN.store(true, Ordering::Relaxed);
    PROMPT_ACTIVE.store(true, Ordering::Relaxed);
    do_print_prompt();
}

pub(crate) fn print_help_hint() {
    println!("Type `help` for commands. Try: play beep");
}

pub(crate) fn print_help() {
    println!("Commands:");
    println!("  help | status");
    println!("  quit");
    println!();
    println!("Audio settings:");
    println!("  mode <auto|stereo|hrtf|surround>");
    println!("  distance <none|inverse|inverse-clamp|linear|linear-clamp|exponent|exponent-clamp>");
    println!("  mute <on|off>");
    println!();
    println!("Buffers:");
    println!("  load <name> <path>");
    println!("  gen <name> <sine|noise> <seconds> [freq_hz]");
    println!();
    println!("Playback:");
    println!("  play <name> [x y z] [gain] [pitch]");
    println!("    Example: play beep 1 0 -2");
    println!("    Note: if you include gain/pitch, supply all coords first");
    println!("  loop <name> [x y z] [gain] [pitch]");
    println!("  loop stop [name]");
    println!();
    println!("Listener:");
    println!("  listener <x y z>");
    println!();
    println!("Orbit:");
    println!("  orbit <name> <radius> <seconds_per_rev>");
    println!("  orbitv <name> <radius> <seconds_per_rev>");
    println!("  orbit <stop>");
    println!();
    println!("Coords are Bevy-style: +X right, +Y up, -Z forward (in front).");
}

pub(crate) fn parse_command(input: &str) -> Result<Command, String> {
    let normalized = normalize_input(input);
    let parts = split(&normalized).map_err(|err| err.to_string())?;
    let Some((head, tail)) = parts.split_first() else {
        return Err("Empty command".to_string());
    };
    match head.as_str() {
        "help" | "h" => Ok(Command::Help),
        "status" => Ok(Command::Status),
        "mode" => parse_mode(tail),
        "distance" => parse_distance(tail),
        "mute" => parse_mute(tail),
        "load" => parse_load(tail),
        "gen" => parse_gen(tail),
        "play" => parse_play(tail),
        "loop" => parse_loop(tail),
        "listener" => parse_listener(tail),
        "orbit" => parse_orbit(tail),
        "orbitv" | "orbit-vertical" => parse_orbit_vertical(tail),
        "quit" | "exit" => Ok(Command::Quit),
        _ => Err(format!("Unknown command: {head}")),
    }
}

pub(crate) struct CommandReceiver {
    rx: Receiver<String>,
    shutdown: Arc<AtomicBool>,
}

impl CommandReceiver {
    pub(crate) fn try_recv(&mut self) -> Result<String, TryRecvError> {
        self.rx.try_recv()
    }

    pub(crate) fn request_shutdown(&mut self) {
        if self.shutdown.swap(true, Ordering::Relaxed) {
            return;
        }
        close_stdin();
    }
}

impl Drop for CommandReceiver {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

pub(crate) struct CommandContext<'a, 'w> {
    runtime: Option<&'a OpenalRuntime>,
    registry: &'a mut sound::BufferRegistry,
    listener_target: &'a mut sound::ListenerTarget,
    orbit: &'a mut sound::OrbitState,
    receiver: &'a mut CommandReceiver,
    loop_tracker: &'a mut sound::LoopTracker,
    exit: &'a mut MessageWriter<'w, AppExit>,
}

impl<'a, 'w> CommandContext<'a, 'w> {
    pub(crate) fn new(
        runtime: Option<&'a OpenalRuntime>,
        registry: &'a mut sound::BufferRegistry,
        listener_target: &'a mut sound::ListenerTarget,
        orbit: &'a mut sound::OrbitState,
        receiver: &'a mut CommandReceiver,
        loop_tracker: &'a mut sound::LoopTracker,
        exit: &'a mut MessageWriter<'w, AppExit>,
    ) -> Self {
        Self {
            runtime,
            registry,
            listener_target,
            orbit,
            receiver,
            loop_tracker,
            exit,
        }
    }
}

pub(crate) fn handle_command(command: Command, ctx: &mut CommandContext<'_, '_>) -> bool {
    match command {
        Command::Help => {
            print_help();
        }
        Command::Status => {
            let Some(runtime) = ctx.runtime else {
                println!("OpenAL runtime unavailable");
                return false;
            };
            print_status(runtime);
        }
        Command::Mode(mode) => {
            let Some(runtime) = ctx.runtime else {
                println!("OpenAL runtime unavailable");
                return false;
            };
            if runtime.runtime().set_render_mode(mode).is_err() {
                println!("Failed to set render mode");
            } else {
                println!("Render mode set to {}", mode.as_str());
                print_status(runtime);
            }
        }
        Command::Distance(model) => {
            let Some(runtime) = ctx.runtime else {
                println!("OpenAL runtime unavailable");
                return false;
            };
            if runtime.runtime().set_distance_model(model).is_err() {
                println!("Failed to set distance model");
            } else {
                println!("Distance model set to {}", model.as_str());
                print_status(runtime);
            }
        }
        Command::Mute(muted) => {
            let Some(runtime) = ctx.runtime else {
                println!("OpenAL runtime unavailable");
                return false;
            };
            if runtime.runtime().set_muted(muted).is_err() {
                println!("Failed to set mute");
            } else {
                println!("Muted: {muted}");
            }
        }
        Command::Load { name, path } => {
            let Some(runtime) = ctx.runtime else {
                println!("OpenAL runtime unavailable");
                return false;
            };
            match std::fs::read(&path) {
                Ok(bytes) => match decode_to_mono_i16(&bytes) {
                    Ok(decoded) => {
                        let key = ctx.registry.allocate_key();
                        if runtime.runtime().create_buffer(key, decoded).is_ok() {
                            let replaced = ctx.registry.insert(name.clone(), key);
                            if let Some(old) = replaced {
                                println!("Loaded {name} as {key} (replacing {old})");
                            } else {
                                println!("Loaded {name} as {key}");
                            }
                        } else {
                            println!("Failed to create buffer for {name}");
                        }
                    }
                    Err(err) => println!("Decode failed: {err}"),
                },
                Err(err) => {
                    if err.kind() == std::io::ErrorKind::NotFound {
                        println!("Read failed: cannot find file at {}", path.display());
                    } else {
                        println!("Read failed for {}: {err}", path.display());
                    }
                }
            }
        }
        Command::Gen {
            name,
            kind,
            seconds,
            freq_hz,
        } => {
            let Some(runtime) = ctx.runtime else {
                println!("OpenAL runtime unavailable");
                return false;
            };
            let decoded = match kind {
                GenKind::Sine => {
                    let freq = freq_hz.unwrap_or(sound::DEFAULT_BEEP_FREQ_HZ);
                    sound::generate_sine(sound::DEFAULT_SAMPLE_RATE_HZ, seconds, freq)
                }
                GenKind::Noise => sound::generate_noise(sound::DEFAULT_SAMPLE_RATE_HZ, seconds),
            };
            let key = ctx.registry.allocate_key();
            if runtime.runtime().create_buffer(key, decoded).is_ok() {
                let replaced = ctx.registry.insert(name.clone(), key);
                if let Some(old) = replaced {
                    println!("Generated {name} as {key} (replacing {old})");
                } else {
                    println!("Generated {name} as {key}");
                }
            } else {
                println!("Failed to create generated buffer for {name}");
            }
        }
        Command::Play {
            name,
            position,
            gain,
            pitch,
        } => {
            let Some(runtime) = ctx.runtime else {
                println!("OpenAL runtime unavailable");
                return false;
            };
            let Some(key) = ctx.registry.get(&name) else {
                println!("Unknown buffer: {name}");
                return false;
            };
            let params = PlayOneShotParams {
                position,
                gain,
                pitch,
            };
            if runtime.runtime().play_one_shot(key, params).is_err() {
                println!("Failed to play {name}");
            }
        }
        Command::LoopStart {
            name,
            position,
            gain,
            pitch,
        } => {
            let Some(runtime) = ctx.runtime else {
                println!("OpenAL runtime unavailable");
                return false;
            };
            let Some(key) = ctx.registry.get(&name) else {
                println!("Unknown buffer: {name}");
                return false;
            };
            if let Some(current) = ctx.loop_tracker.current_name() {
                if current != name {
                    let _ = runtime.runtime().stop_loop();
                }
            }
            let params = PlayOneShotParams {
                position,
                gain,
                pitch,
            };
            if runtime.runtime().start_loop(key, params).is_err() {
                println!("Failed to start loop for {name}");
            } else {
                ctx.loop_tracker.set_name(Some(name.clone()));
                println!("Looping {name}");
            }
        }
        Command::LoopStop { name } => {
            let Some(runtime) = ctx.runtime else {
                println!("OpenAL runtime unavailable");
                return false;
            };
            let current = ctx.loop_tracker.current_name();
            if let Some(requested) = name.as_deref() {
                if current.is_none() {
                    println!("No active loop");
                    return false;
                }
                if current != Some(requested) {
                    println!("Active loop is '{}'", current.unwrap_or("none"));
                    return false;
                }
            } else if current.is_none() {
                println!("No active loop");
                return false;
            }

            if runtime.runtime().stop_loop().is_err() {
                println!("Failed to stop loop");
            } else {
                ctx.loop_tracker.clear();
                println!("Stopped loop");
            }
        }
        Command::Listener { position } => {
            ctx.listener_target.set_position(position);
            println!("Listener position set to {position:?}");
        }
        Command::Orbit {
            name,
            radius,
            seconds_per_rev,
        } => {
            let Some(key) = ctx.registry.get(&name) else {
                println!("Unknown buffer: {name}");
                return false;
            };
            ctx.orbit.start_horizontal(key, radius, seconds_per_rev);
            println!("Orbit enabled");
        }
        Command::OrbitVertical {
            name,
            radius,
            seconds_per_rev,
        } => {
            let Some(key) = ctx.registry.get(&name) else {
                println!("Unknown buffer: {name}");
                return false;
            };
            ctx.orbit.start_vertical(key, radius, seconds_per_rev);
            println!("Vertical orbit enabled");
        }
        Command::OrbitStop => {
            ctx.orbit.stop();
            println!("Orbit stopped");
        }
        Command::Quit => {
            ctx.receiver.request_shutdown();
            ctx.exit.write(AppExit::Success);
            return true;
        }
    }
    false
}

pub(crate) enum Command {
    Help,
    Status,
    Mode(AudioRenderMode),
    Distance(DistanceModel),
    Mute(bool),
    Load {
        name: String,
        path: PathBuf,
    },
    Gen {
        name: String,
        kind: GenKind,
        seconds: f32,
        freq_hz: Option<f32>,
    },
    Play {
        name: String,
        position: Vec3,
        gain: f32,
        pitch: f32,
    },
    LoopStart {
        name: String,
        position: Vec3,
        gain: f32,
        pitch: f32,
    },
    LoopStop {
        name: Option<String>,
    },
    Listener {
        position: Vec3,
    },
    Orbit {
        name: String,
        radius: f32,
        seconds_per_rev: f32,
    },
    OrbitVertical {
        name: String,
        radius: f32,
        seconds_per_rev: f32,
    },
    OrbitStop,
    Quit,
}

pub(crate) enum GenKind {
    Sine,
    Noise,
}

fn install_ctrlc_handler() {
    let _ = ctrlc::set_handler(move || {
        CTRL_C_REQUESTED.store(true, Ordering::Relaxed);
    });
}

fn read_stdin(tx: mpsc::Sender<String>, shutdown: Arc<AtomicBool>) {
    let stdin = std::io::stdin();
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let line = line.trim().to_string();
                mark_prompt_inactive();
                if tx.send(line).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn close_stdin() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Console::{GetStdHandle, STD_INPUT_HANDLE};

        let handle = GetStdHandle(STD_INPUT_HANDLE);
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            let _ = CloseHandle(handle);
        }
    }
    #[cfg(not(windows))]
    unsafe {
        let _ = libc::close(0);
    }
}

fn normalize_input(input: &str) -> String {
    if cfg!(windows) {
        input.replace('\\', "/")
    } else {
        input.to_string()
    }
}

fn parse_mode(args: &[String]) -> Result<Command, String> {
    let Some(value) = args.first() else {
        return Err("mode <auto|stereo|hrtf|surround>".to_string());
    };
    let mode = AudioRenderMode::parse(value)
        .ok_or_else(|| "mode <auto|stereo|hrtf|surround>".to_string())?;
    Ok(Command::Mode(mode))
}

fn parse_distance(args: &[String]) -> Result<Command, String> {
    let Some(value) = args.first() else {
        return Err(
            "distance <none|inverse|inverse-clamp|linear|linear-clamp|exponent|exponent-clamp>"
                .to_string(),
        );
    };
    let model = DistanceModel::parse(value).ok_or_else(|| {
        "distance <none|inverse|inverse-clamp|linear|linear-clamp|exponent|exponent-clamp>"
            .to_string()
    })?;
    Ok(Command::Distance(model))
}

fn parse_mute(args: &[String]) -> Result<Command, String> {
    let Some(value) = args.first() else {
        return Err("mute <on|off>".to_string());
    };
    let muted = match value.as_str() {
        "on" | "true" | "1" => true,
        "off" | "false" | "0" => false,
        _ => return Err("mute <on|off>".to_string()),
    };
    Ok(Command::Mute(muted))
}

fn parse_load(args: &[String]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err("load <name> <path>".to_string());
    }
    Ok(Command::Load {
        name: args[0].clone(),
        path: PathBuf::from(&args[1]),
    })
}

fn parse_gen(args: &[String]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err("gen <name> <sine|noise> <seconds> [freq_hz]".to_string());
    }
    let kind = match args[1].as_str() {
        "sine" => GenKind::Sine,
        "noise" => GenKind::Noise,
        _ => return Err("gen <name> <sine|noise> <seconds> [freq_hz]".to_string()),
    };
    let seconds = parse_f32(&args[2])?;
    let freq_hz = if args.len() > 3 {
        Some(parse_f32(&args[3])?)
    } else {
        None
    };
    Ok(Command::Gen {
        name: args[0].clone(),
        kind,
        seconds,
        freq_hz,
    })
}

fn parse_play(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("play <name> [x y z] [gain] [pitch]".to_string());
    }
    let name = args[0].clone();
    let (position, gain, pitch) = parse_play_params(&args[1..])?;
    Ok(Command::Play {
        name,
        position,
        gain,
        pitch,
    })
}

fn parse_loop(args: &[String]) -> Result<Command, String> {
    if args.is_empty() {
        return Err("loop <name> [x y z] [gain] [pitch] | loop stop [name]".to_string());
    }
    if args[0] == "stop" || args[0] == "off" {
        let name = args.get(1).cloned();
        return Ok(Command::LoopStop { name });
    }

    let name = args[0].clone();
    let (position, gain, pitch) = parse_play_params(&args[1..])?;
    Ok(Command::LoopStart {
        name,
        position,
        gain,
        pitch,
    })
}

fn parse_play_params(numbers: &[String]) -> Result<(Vec3, f32, f32), String> {
    let mut position = Vec3::new(0.0, 0.0, -2.0);
    let mut gain = 1.0;
    let mut pitch = 1.0;

    match numbers.len() {
        0 => {}
        1 => {
            let x = parse_f32(&numbers[0])?;
            position.x = x;
        }
        2 => {
            let x = parse_f32(&numbers[0])?;
            let y = parse_f32(&numbers[1])?;
            position.x = x;
            position.y = y;
        }
        3 => {
            let x = parse_f32(&numbers[0])?;
            let y = parse_f32(&numbers[1])?;
            let z = parse_f32(&numbers[2])?;
            position = Vec3::new(x, y, z);
        }
        4 => {
            let x = parse_f32(&numbers[0])?;
            let y = parse_f32(&numbers[1])?;
            let z = parse_f32(&numbers[2])?;
            position = Vec3::new(x, y, z);
            gain = parse_f32(&numbers[3])?;
        }
        5 => {
            let x = parse_f32(&numbers[0])?;
            let y = parse_f32(&numbers[1])?;
            let z = parse_f32(&numbers[2])?;
            position = Vec3::new(x, y, z);
            gain = parse_f32(&numbers[3])?;
            pitch = parse_f32(&numbers[4])?;
        }
        _ => {
            return Err("play <name> [x y z] [gain] [pitch]".to_string());
        }
    }

    if gain < 0.0 {
        return Err("gain must be >= 0".to_string());
    }
    if pitch < 0.0 {
        return Err("pitch must be >= 0".to_string());
    }

    Ok((position, gain, pitch))
}

fn parse_listener(args: &[String]) -> Result<Command, String> {
    if args.len() != 3 {
        return Err("listener <x y z>".to_string());
    }
    let x = parse_f32(&args[0])?;
    let y = parse_f32(&args[1])?;
    let z = parse_f32(&args[2])?;
    Ok(Command::Listener {
        position: Vec3::new(x, y, z),
    })
}

fn parse_orbit(args: &[String]) -> Result<Command, String> {
    if args.len() == 1 && (args[0] == "off" || args[0] == "stop") {
        return Ok(Command::OrbitStop);
    }
    if args.len() != 3 {
        return Err("orbit <name> <radius> <seconds_per_rev> | orbit <stop>".to_string());
    }
    let radius = parse_f32(&args[1])?;
    let seconds_per_rev = parse_f32(&args[2])?;
    Ok(Command::Orbit {
        name: args[0].clone(),
        radius,
        seconds_per_rev,
    })
}

fn parse_orbit_vertical(args: &[String]) -> Result<Command, String> {
    if args.len() == 1 && (args[0] == "off" || args[0] == "stop") {
        return Ok(Command::OrbitStop);
    }
    if args.len() != 3 {
        return Err("orbitv <name> <radius> <seconds_per_rev> | orbitv <stop>".to_string());
    }
    let radius = parse_f32(&args[1])?;
    let seconds_per_rev = parse_f32(&args[2])?;
    Ok(Command::OrbitVertical {
        name: args[0].clone(),
        radius,
        seconds_per_rev,
    })
}

fn parse_f32(value: &str) -> Result<f32, String> {
    let parsed = value
        .parse::<f32>()
        .map_err(|_| format!("Invalid number: {value}"))?;
    if !parsed.is_finite() {
        return Err(format!("Invalid number: {value}"));
    }
    Ok(parsed)
}

fn print_status(runtime: &OpenalRuntime) {
    let status = runtime.runtime().status();
    let output_mode = status.output_mode.as_deref().unwrap_or("unknown");
    let last_error = status.last_error.as_deref().unwrap_or("none");

    println!("render_mode: {}", status.render_mode.as_str());
    match status.output_mode_raw {
        Some(raw) => println!("output_mode: {output_mode} (raw=0x{raw:04X})"),
        None => println!("output_mode: {output_mode}"),
    }
    println!("distance_model: {}", status.distance_model.as_str());
    println!("hrtf_active: {}", status.hrtf_active);
    println!("muted: {}", status.muted);
    println!("buffers: {}", status.loaded_buffers);
    println!("sources: {}", status.active_sources);
    println!("last_error: {last_error}");
}

fn print_prompt_once() {
    if PROMPT_SHOWN.swap(true, Ordering::Relaxed) {
        return;
    }
    PROMPT_ACTIVE.store(true, Ordering::Relaxed);
    do_print_prompt();
}

fn mark_prompt_inactive() {
    PROMPT_ACTIVE.store(false, Ordering::Relaxed);
}

fn do_print_prompt() {
    let mut stdout = std::io::stdout();
    print!("> ");
    let _ = stdout.flush();
}

struct PromptingWriter {
    inner: std::io::Stderr,
}

impl PromptingWriter {
    fn new() -> Self {
        Self {
            inner: std::io::stderr(),
        }
    }
}

impl Write for PromptingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buf)?;
        if PROMPT_ACTIVE.load(Ordering::Relaxed) && buf[..written].contains(&b'\n') {
            print_prompt();
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
