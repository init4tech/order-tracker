use git_version::git_version;
use init4_bin_base::deps::tracing::{info, info_span};
use signet_tracker_server::config::env_var_info;
use std::env;

const GIT_COMMIT: &str =
    git_version!(args = ["--always", "--match=", "--abbrev=7"], fallback = "unknown");
const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    let env_vars = env_var_info();
    println!(
        r#"Signet order tracker server v{PKG_VERSION}

Run with no args. The process will run until it receives a SIGTERM or SIGINT signal.

Configuration is via the following environment variables:
{env_vars}
"#
    );
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> eyre::Result<()> {
    if env::args().any(|arg| arg == "-h" || arg == "--help") {
        print_help();
        return Ok(());
    }

    let config_and_guard = signet_tracker_server::config_from_env()?;
    let config = &config_and_guard.config;
    let init_span = info_span!("tracker initialization").entered();

    info!(pkg_version = PKG_VERSION, git_commit = GIT_COMMIT, "starting tracker server");

    let cancellation_token = signet_tracker_server::handle_signals()?;
    let tasks = signet_tracker_server::run(config, cancellation_token).await?;

    drop(init_span);

    tasks.await
}
