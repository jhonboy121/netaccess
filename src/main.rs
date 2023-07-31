mod account_manager;
mod monitor;
#[cfg(target_family = "unix")]
mod openssl_conf;
mod user;

use account_manager::{AccountManager, Connection};
use anyhow::{bail, Context};
use chrono::Duration;
use clap::{Parser, Subcommand, ValueEnum};
use crossterm::{
    cursor::{RestorePosition, SavePosition},
    execute,
    terminal::{Clear, ClearType},
    ExecutableCommand,
};
use monitor::{Message, Monitor};
use std::{
    fmt::{self, Display, Formatter},
    io::{self, Write},
    net::IpAddr,
    sync::Arc,
    time,
};
use tokio::sync::mpsc;
use user::User;

const MIN_SUSPEND_DURATION: u64 = 30;
const MSG_CHANNEL_BUF_SIZE: usize = 20;

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

        /// The duration for which an IP address should be approved for
        #[arg(short, long, default_value_t = ApproveDuration::Hour, value_enum)]
        approve_duration: ApproveDuration,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterruptKey {
    Retry,
    Wakeup,
}

impl TryFrom<&str> for InterruptKey {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value.to_lowercase().trim() {
            "r" => Ok(Self::Retry),
            "w" => Ok(Self::Wakeup),
            other => Err(format!("Unknown interrupt key {other}")),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    #[cfg(target_family = "unix")]
    let _cnf = openssl_conf::OpenSSLConf::new()?;

    let cli = Cli::parse();
    let account_manager = Arc::new(AccountManager::new()?);

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
            approve_duration,
        } => {
            if suspend_duration < MIN_SUSPEND_DURATION {
                bail!("Suspend duration is less than minimum allowed {MIN_SUSPEND_DURATION}");
            }
            io::stdout().execute(SavePosition)?;
            let user = get_user()?;
            let mut monitor = Monitor::new(&account_manager);
            let (message_sender, message_receiver) = mpsc::channel(MSG_CHANNEL_BUF_SIZE);
            monitor.start(
                user,
                approve_duration.into(),
                time::Duration::from_secs(suspend_duration),
                message_sender,
            );
            handle_monitor_messages(message_receiver).await?;
        }
    }

    Ok(())
}

async fn display_status(account_manager: &AccountManager, user: &User) -> anyhow::Result<()> {
    let status = account_manager.status(user).await?;
    let (ip, connection) = status.system_connection();
    println!("{}", create_connection_status_string(ip, connection));
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

fn create_connection_status_string(ip: &IpAddr, connection: &Connection) -> String {
    format!(
        "Your IP address is {ip} and {}",
        if connection.is_active() {
            format!("active for {}", format_duration(connection.time_left()))
        } else {
            String::from("inactive")
        }
    )
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

async fn handle_monitor_messages(mut receiver: mpsc::Receiver<Message>) -> anyhow::Result<()> {
    let restore_cursor = || {
        execute!(
            io::stdout(),
            RestorePosition,
            Clear(ClearType::FromCursorDown),
            SavePosition
        )
    };
    restore_cursor()?;

    let listen_for_interrupt = |interrupt_key: InterruptKey| async move {
        loop {
            let input = tokio::task::spawn_blocking(|| {
                let mut buf = String::new();
                io::stdin().read_line(&mut buf).map(|_| buf)
            })
            .await??;
            if InterruptKey::try_from(&*input).is_ok_and(|key| key == interrupt_key) {
                break;
            }
        }
        tokio::io::Result::Ok(())
    };

    while let Some(msg) = receiver.recv().await {
        match msg {
            Message::Suspended {
                duration,
                wake_sender,
            } => {
                restore_cursor()?;
                println!("Suspended for {duration:?}");
                println!("Enter 'W' (case insensitive) and hit Enter key to wakeup monitor");
                listen_for_interrupt(InterruptKey::Wakeup).await?;
                if wake_sender.send(()).is_err() {
                    bail!("Monitor channel abruptly clossed, exiting");
                }
            }
            Message::CheckingStatus => {
                restore_cursor()?;
                println!("Checking status");
            }
            Message::Status { ip, connection } => {
                restore_cursor()?;
                println!("{}", create_connection_status_string(&ip, &connection));
                if connection.is_active() {
                    // We want the user to know about current active IP.
                    execute!(io::stdout(), SavePosition)?;
                }
            }
            Message::Approving(ip) => {
                restore_cursor()?;
                println!("Approved IP {ip}")
            }
            Message::Error {
                error,
                retry_sender,
            } => {
                restore_cursor()?;
                println!("Monitoring failed : {error}");
                println!("Enter 'R' (case insensitive) and hit Enter key to restart monitor");
                listen_for_interrupt(InterruptKey::Retry).await?;
                if retry_sender.send(()).is_err() {
                    bail!("Retry channel abruptly clossed, exiting");
                }
            }
        }
    }
    Ok(())
}
