mod app;
mod auth;
mod backend;
mod config;
mod gcal;
mod local;
mod models;
mod ui;

use anyhow::{Context, Result};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("calendar-cli v{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args
        .iter()
        .any(|a| a == "--help" || a == "-h" || a == "help")
    {
        print_usage();
        return Ok(());
    }

    if args.iter().any(|a| a == "--login" || a == "login") {
        return cmd_login().await;
    }

    let config = config::Config::load()?;

    let backend: Box<dyn backend::CalendarBackend> = if config.token_path.exists() {
        let auth =
            auth::GoogleAuth::load(&config.credentials_path, config.token_path.clone()).await?;
        if !auth.needs_auth() {
            println!("Connected to Google Calendar");
            Box::new(gcal::GoogleCalendar::new(auth))
        } else {
            println!("Session expired — sign in again from Settings");
            Box::new(local::LocalCalendar::new(config.events_path()))
        }
    } else {
        Box::new(local::LocalCalendar::new(config.events_path()))
    };

    let mut terminal = ratatui::init();
    let mut app = app::App::new(backend, &config);
    let result = app.run(&mut terminal).await;
    ratatui::restore();
    result
}

async fn cmd_login() -> Result<()> {
    let config = config::Config::load()?;

    println!("Signing in to Google Calendar...");
    let auth = auth::GoogleAuth::load(&config.credentials_path, config.token_path.clone()).await?;
    let mut gcal = gcal::GoogleCalendar::new(auth);

    if !gcal.needs_auth() {
        println!(
            "Already authenticated. Token saved at: {}",
            config.token_path.display()
        );
        return Ok(());
    }

    gcal.authenticate().await.context("Google sign-in failed")?;
    println!("✓ Successfully signed in to Google Calendar!");
    println!("  Token saved to: {}", config.token_path.display());

    Ok(())
}

fn print_usage() {
    let name = std::env::args()
        .next()
        .unwrap_or_else(|| "calendar-cli".into());
    eprintln!("calendar-cli v{}", env!("CARGO_PKG_VERSION"));
    eprintln!("Usage:");
    eprintln!("  {name}                    Start the TUI calendar app");
    eprintln!("  {name} --login            Re-authenticate with Google Calendar");
    eprintln!("  {name} --version / -V     Show version");
    eprintln!("  {name} --help / -h        Show this help");
}
