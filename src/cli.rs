//! Command-line interface for Adlib
//!
//! Handles argument parsing and logging configuration.

use clap::Parser;
use log::LevelFilter;

/// Adlib - Voice recorder and transcription application
#[derive(Parser, Debug)]
#[command(name = "adlib")]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Increase logging verbosity
    /// -v = info, -vv = debug, -vvv = trace (includes whisper), -vvvv = all deps
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress all output except errors
    #[arg(short, long)]
    pub quiet: bool,
}

impl Args {
    /// Get the log level filter based on verbosity flags
    pub fn log_level(&self) -> LevelFilter {
        if self.quiet {
            LevelFilter::Error
        } else {
            match self.verbose {
                0 => LevelFilter::Warn,
                1 => LevelFilter::Info,
                2 => LevelFilter::Debug,
                _ => LevelFilter::Trace,
            }
        }
    }

    /// Check if whisper verbose output should be enabled
    /// Only at trace level (-vvv) do we show whisper internals
    pub fn whisper_verbose(&self) -> bool {
        self.verbose >= 3
    }
}

/// Initialize the logging system based on CLI arguments
pub fn init_logging(args: &Args) {
    let mut builder = env_logger::Builder::new();

    // Base level for all modules - keep at warn to suppress noisy deps
    builder.filter_level(LevelFilter::Warn);

    // Set adlib modules to requested verbosity level
    builder.filter_module("adlib", args.log_level());

    // Whisper output (via our custom callback) only at -vvv
    if args.whisper_verbose() {
        builder.filter_module("whisper", args.log_level());
    }

    // GUI framework modules only at -vvvv (very verbose)
    if args.verbose >= 4 {
        builder.filter_module("naga", args.log_level());
        builder.filter_module("blade_graphics", args.log_level());
        builder.filter_module("gpui", args.log_level());
        builder.filter_module("fontdb", args.log_level());
    }

    builder.format_timestamp_millis().init();
}
