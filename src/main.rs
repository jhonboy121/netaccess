mod account_manager;
mod user;

use account_manager::AccountManager;
use anyhow::{Context, Result};
use chrono::Duration;
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    env,
    fmt::{self, Display, Formatter},
};
use user::User;

#[derive(Debug, Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Status {
        #[arg(short, long)]
        user: String,
    },
    Approve {
        #[arg(short, long)]
        user: String,

        #[arg(short, long, default_value_t = ApproveDuration::Hour, value_enum)]
        duration: ApproveDuration,

        #[arg(short, long, default_value_t = false)]
        force: bool,
    },
    Revoke {
        #[arg(short, long)]
        user: String,

        #[arg(short, long)]
        ip: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ApproveDuration {
    Hour,
    Day,
    Month,
}

impl Display for ApproveDuration {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.to_possible_value()
            .expect("No values are skipped")
            .get_name()
            .fmt(f)
    }
}

impl From<ApproveDuration> for usize {
    fn from(val: ApproveDuration) -> Self {
        match val {
            ApproveDuration::Hour => 1,
            ApproveDuration::Day => 2,
            ApproveDuration::Month => 3,
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    #[cfg(target_os = "linux")]
    env::set_var("OPENSSL_CONF", "openssl.conf");

    let cli = Cli::parse();
    let account_manager = AccountManager::new()?;

    let get_user = |user| {
        let password = rpassword::prompt_password(format!("Enter password for {user}:"))
            .context("Failed to read password")?;
        Ok::<User, anyhow::Error>(User::new(user, password))
    };

    match cli.command {
        Command::Status { user } => display_status(&account_manager, &get_user(user)?).await?,
        Command::Approve {
            user,
            duration,
            force,
        } => {
            let user = get_user(user)?;
            let ip = account_manager
                .approve(&user, duration.into(), force)
                .await?;
            println!("Approved {ip} for {user} for 1 {duration} successfully");
        }
        Command::Revoke { user, ip } => {
            let user = get_user(user)?;
            let ip = account_manager.revoke(&user, ip).await?;
            println!("Revoked {ip} for {user} successfully");
        }
    }

    Ok(())
}

async fn display_status(account_manager: &AccountManager, user: &User) -> Result<()> {
    let status = account_manager.status(user).await?;
    let (ip, connection) = status.system_connection();
    println!(
        "Your IP address is {ip} and {}",
        if connection.is_active() {
            format!("active for {}", format_duration(connection.time_left()))
        } else {
            String::from("inactive")
        }
    );
    let connections = status.connections();
    println!(
        "Number of other registered connections: {}",
        connections.len()
    );
    if !connections.is_empty() {
        println!("S.No.\tIP\t\tTime left");
    }
    for (index, (ip, connection)) in connections.iter().enumerate() {
        println!(
            "{}\t{ip}\t{}",
            index + 1,
            if connection.is_active() {
                format_duration(connection.time_left())
            } else {
                String::from("Inactive or expired")
            }
        );
    }
    Ok(())
}

fn format_duration(duration: &Duration) -> String {
    let mut fragments = Vec::with_capacity(3);
    macro_rules! push {
        ( $unit:expr ) => {
            fragments.push(format!("{} {}", $unit, stringify!($unit)));
        };
    }
    let minutes = duration.num_minutes() % 60;
    if minutes > 0 {
        push!(minutes);
    }
    let hours = duration.num_hours() % 24;
    if hours > 0 {
        push!(hours);
    }
    let days = duration.num_days();
    if days > 0 {
        push!(days);
    }
    fragments
        .into_iter()
        .rev()
        .collect::<Vec<String>>()
        .join(", ")
}
