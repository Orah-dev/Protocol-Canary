//! Entry point for the `stellar-canary` command-line interface.

mod cli;
mod commands;
mod network;

use clap::Parser;

use cli::{Cli, Command};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // `diff` follows CF-05's dedicated gating exit codes (0/1/2, see
    // `commands::DiffExitCode`) rather than `canary_core::ExitCode`, so
    // it computes the raw process code itself.
    let exit_code = match cli.command {
        Command::Check(args) => commands::run_check(args).await.code(),
        Command::Inspect(args) => commands::run_inspect(args).code(),
        Command::Fixtures(args) => commands::run_fixtures(args).code(),
        Command::Report(args) => commands::run_report(args).code(),
        Command::Diff(args) => commands::run_diff(args),
        Command::Lock(args) => commands::run_lock(args).code(),
        Command::Version => commands::run_version().code(),
    };

    std::process::exit(exit_code);
}
