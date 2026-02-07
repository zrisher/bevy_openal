use crate::cli;
use crate::sound;
use bevy_app::{App, AppExit, ScheduleRunnerPlugin, Startup, Update};
use bevy_ecs::message::MessageWriter;
use bevy_ecs::prelude::{NonSendMut, ResMut};
use bevy_openal::{BevyOpenalPlugin, OpenalRuntime, OpenalSettings};
use bevy_time::TimePlugin;
use bevy_transform::prelude::TransformPlugin;
use std::time::Duration;

const RUN_LOOP_MS: u64 = 16;

pub(crate) fn run(command_receiver: cli::CommandReceiver) {
    App::new()
        .add_message::<AppExit>()
        .insert_non_send_resource(command_receiver)
        .insert_resource(sound::BufferRegistry::default())
        .insert_resource(sound::ListenerTarget::default())
        .insert_resource(sound::OrbitState::default())
        .insert_resource(sound::LoopTracker::default())
        .insert_resource(sound::DefaultSampleState::default())
        .add_plugins(TimePlugin)
        .add_plugins(TransformPlugin)
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(
            RUN_LOOP_MS,
        )))
        .add_plugins(BevyOpenalPlugin)
        .insert_resource(OpenalSettings::default())
        .add_systems(Startup, sound::setup_listener)
        .add_systems(
            Update,
            (
                cli::handle_ctrlc_exit,
                sound::ensure_default_sample,
                sound::apply_listener_target,
                handle_commands,
                sound::update_orbit,
                cli::ensure_prompt_visible,
            ),
        )
        .run();
}

fn handle_commands(
    mut receiver: NonSendMut<cli::CommandReceiver>,
    mut runtime: Option<ResMut<OpenalRuntime>>,
    mut registry: ResMut<sound::BufferRegistry>,
    mut listener_target: ResMut<sound::ListenerTarget>,
    mut orbit: ResMut<sound::OrbitState>,
    mut exit: MessageWriter<AppExit>,
    mut loop_tracker: ResMut<sound::LoopTracker>,
) {
    while let Ok(line) = receiver.try_recv() {
        let line = line.trim();
        if line.is_empty() {
            cli::print_prompt();
            continue;
        }
        let mut should_exit = false;
        match cli::parse_command(line) {
            Ok(command) => {
                let mut ctx = cli::CommandContext::new(
                    runtime.as_deref(),
                    &mut registry,
                    &mut listener_target,
                    &mut orbit,
                    &mut receiver,
                    &mut loop_tracker,
                    &mut exit,
                );
                should_exit = cli::handle_command(command, &mut ctx);
            }
            Err(err) => {
                println!("Command error: {err}");
            }
        }
        if should_exit {
            if let Some(runtime) = runtime.as_deref_mut() {
                runtime.shutdown();
            }
            break;
        }
        cli::print_prompt();
    }
}
