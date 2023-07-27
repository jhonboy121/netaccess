mod account_manager;
#[cfg(target_family = "unix")]
mod openssl_conf;
mod user;

use account_manager::AccountManager;
use anyhow::{bail, Context, Result};
use chrono::Duration;
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    fmt::{self, Display, Formatter},
    io::{self, Write},
    time,
};
use user::User;

const MIN_SUSPEND_DURATION: u64 = 30;

#[derive(Debug, Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Query the status of a user account
    Status,
    /// Approve system IP address for a particular duration
    Approve {
        /// The duration for which an IP address should be approved for
        #[arg(short, long, default_value_t = ApproveDuration::Hour, value_enum)]
        duration: ApproveDuration,

        /// Forcefully attempt to approve even if system IP is marked as active
        #[arg(short, long, default_value_t = false)]
        force: bool,
    },
    /// Revoke authorization of an IP address
    Revoke {
        /// The IP address for which access should be revoked. Do not specify this flag to revoke
        /// access for your system's IP address
        #[arg(short, long)]
        ip: Option<String>,
    },
    /// Periodically monitor the status of system IP address and approve if access is revoked
    Monitor {
        /// The duration of time in seconds to sleep before waking up to check status
        #[arg(short, long, default_value_t = 5 * 60)]
        suspend_duration: u64,

        /// Pass this flag to disable all status logs
        #[arg(short, long)]
        quiet: bool,
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
    #[cfg(target_family = "unix")]
    let _cnf = openssl_conf::OpenSSLConf::new()?;

    let cli = Cli::parse();
    let account_manager = AccountManager::new()?;

    let get_user = || {
        print!("Enter username: ");
        io::stdout().flush()?;
        // user names are expected to be of the format XX19X001
        let mut buf = String::with_capacity(8);
        io::stdin()
            .read_line(&mut buf)
            .context("Failed to read username")?;
        let user = buf.trim();
        let password = rpassword::prompt_password(format!("Enter password for {user}: "))
            .context("Failed to read password")?;
        anyhow::Ok(User::new(user.to_owned(), password))
    };

    match cli.command {
        Command::Status => display_status(&account_manager, &get_user()?).await?,
        Command::Approve { duration, force } => {
            let user = get_user()?;
            let ip = account_manager
                .approve(&user, duration.into(), force)
                .await?;
            println!("Approved {ip} for {user} for 1 {duration} successfully");
        }
        Command::Revoke { ip } => {
            let user = get_user()?;
            let ip = account_manager.revoke(&user, ip).await?;
            println!("Revoked {ip} for {user} successfully");
        }
        Command::Monitor {
            suspend_duration,
            quiet,
        } => {
            if suspend_duration < MIN_SUSPEND_DURATION {
                bail!("Suspend duration is less than minimum allowed {MIN_SUSPEND_DURATION}");
            }
            let user = get_user()?;
            monitor(
                &user,
                &account_manager,
                time::Duration::from_secs(suspend_duration),
                quiet,
            )
            .await?;
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

async fn monitor(
    user: &User,
    account_manager: &AccountManager,
    suspend_duration: time::Duration,
    quiet: bool,
) -> Result<()> {
    macro_rules! println_checked {
        ( $($arg:tt)* ) => {
            if !quiet {
                println!($( $arg )*);
            }
        };
    }
    println_checked!("Entering monitor mode");
    loop {
        let status = account_manager.status(user).await?;
        let (ip, connection) = status.system_connection();
        if !connection.is_active() {
            println_checked!("IP {ip} is not active, approving...");
            account_manager
                .approve(user, ApproveDuration::Day.into(), false)
                .await?;
            println_checked!("Approved {ip} for 1 {}", ApproveDuration::Day);
        } else {
            println_checked!("IP {ip} is active, suspending for {suspend_duration:?}");
            tokio::time::sleep(suspend_duration).await
        }
    }
}
