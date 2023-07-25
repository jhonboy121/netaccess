mod account_manager;
mod user;

use account_manager::AccountManager;
use anyhow::Result;
use chrono::Duration;
use clap::{Args, Parser, Subcommand, ValueEnum};
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
    Status(UserArgs),
    Approve {
        #[command(flatten)]
        user: UserArgs,

        #[arg(short, long, default_value_t = ApproveDuration::Hour, value_enum)]
        duration: ApproveDuration,

        #[arg(short, long, default_value_t = false)]
        force: bool,
    },
    Revoke {
        #[command(flatten)]
        user: UserArgs,

        #[arg(short, long)]
        ip: Option<String>,
    },
}

#[derive(Debug, Args, Clone)]
struct UserArgs {
    #[arg(short, long)]
    name: String,

    #[arg(short, long)]
    password: String,
}

impl Into<User> for UserArgs {
    fn into(self) -> User {
        User::new(self.name, self.password)
    }
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

impl Into<usize> for ApproveDuration {
    fn into(self) -> usize {
        match self {
            ApproveDuration::Hour => 1,
            ApproveDuration::Day => 2,
            ApproveDuration::Month => 3,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    #[cfg(target_os = "linux")]
    env::set_var("OPENSSL_CONF", "openssl.conf");

    let cli = Cli::parse();
    let account_manager = AccountManager::new()?;

    match cli.command {
        Command::Status(user) => display_status(&account_manager, &user.into()).await?,
        Command::Approve {
            user,
            duration,
            force,
        } => {
            let user = user.into();
            let ip = account_manager
                .approve(&user, duration.into(), force)
                .await?;
            println!("Approved {ip} for {user} for 1 {duration} successfully");
        }
        Command::Revoke { user, ip } => {
            let user = user.into();
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
    let mut strs = Vec::with_capacity(3);
    let minutes = duration.num_minutes() % 60;
    if minutes > 0 {
        strs.push(format!("{} minutes", minutes));
    }
    let hours = duration.num_hours() % 24;
    if hours > 0 {
        strs.push(format!("{} hours", hours));
    }
    let days = duration.num_days();
    if days > 0 {
        strs.push(format!("{days} days"));
    }
    let strs: Vec<String> = strs.into_iter().rev().collect();
    strs.join(", ")
}
