//! Entry point for the `stellar-canary` command-line interface.

mod cli;
mod commands;
mod error;
mod fixtures_json;
mod inspect_json;
mod network;

use clap::Parser;

use cli::{CacheArgs, Cli, Command};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // `diff` follows CF-05's dedicated gating exit codes (0/1/2, see
    // `commands::DiffExitCode`) rather than `canary_core::ExitCode`, so
    // it computes the raw process code itself.
    let exit_code = match cli.command {
        Command::Check(args) => commands::run_check(args).await,
        Command::Inspect(args) => commands::run_inspect(args),
        Command::Fixtures(args) => commands::run_fixtures(args),
        Command::Report(args) => commands::run_report(args),
        Command::Cache(CacheArgs::Clear(args)) => commands::run_cache_clear(args),
        Command::Completions(args) => commands::run_completions(args),
        Command::Version => commands::run_version(),
    };

    std::process::exit(exit_code);
}
