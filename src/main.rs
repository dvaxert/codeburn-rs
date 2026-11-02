use clap::{Parser, Subcommand};

mod parse;
mod payload;
mod price;
mod quota;
mod report;
mod scan;
mod usage;

#[derive(Parser)]
#[command(
    name = "codeburn-rs",
    version,
    about = "Local spend from Claude, Codex, and Cursor session files"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print a period as JSON for the panel.
    Status {
        /// Output format. Only `menubar-json` is implemented.
        #[arg(long, default_value = "menubar-json")]
        format: String,
        /// `today`, `week`, `30days`, `month`, or `all`.
        #[arg(long, default_value = "today")]
        period: String,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Status { format, period } => {
            if format != "menubar-json" {
                eprintln!("unknown format {format:?}; only menubar-json is implemented");
                std::process::exit(2);
            }
            let Some(period) = payload::Period::parse(&period) else {
                eprintln!("unknown period {period:?}; expected today, week, 30days, month, or all");
                std::process::exit(2);
            };
            let roots = scan::Roots::from_env();
            let limits = quota::fetch(&roots);
            let body = payload::menubar_json(
                &scan::scan(&roots),
                &parse::collect(&roots),
                period,
                &limits,
            );
            println!("{body}");
        }
    }
}
