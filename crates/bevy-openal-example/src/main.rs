mod app;
mod cli;
mod sound;

fn main() {
    cli::init_logging();
    let command_receiver = cli::start_command_receiver();
    app::run(command_receiver);
}
