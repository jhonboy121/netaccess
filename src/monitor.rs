use crate::{
    account_manager::{AccountManager, SystemStatus},
    user::User,
};
use anyhow::Context;
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio::{
    select,
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
    time,
};

#[derive(Debug)]
pub enum State {
    Suspended {
        duration: Duration,
        wake_sender: oneshot::Sender<()>,
    },
    CheckingStatus,
    Approving(IpAddr),
    Error {
        error: anyhow::Error,
        retry_sender: oneshot::Sender<()>,
    },
}

#[derive(Debug)]
pub struct Monitor {
    handle: Option<JoinHandle<()>>,
    account_manager: Arc<AccountManager>,
}

impl Monitor {
    pub fn new(account_manager: &Arc<AccountManager>) -> Self {
        Self {
            handle: None,
            account_manager: Arc::clone(account_manager),
        }
    }

    pub fn start(
        &mut self,
        user: User,
        duration_index: usize,
        suspend_duration: Duration,
        status_sender: watch::Sender<Option<SystemStatus>>,
        state_sender: mpsc::Sender<State>,
    ) {
        if self.handle.is_some() {
            return;
        }
        let account_manager = Arc::clone(&self.account_manager);
        self.handle = tokio::spawn(async move {
            loop {
                let result = Self::run(
                    &user,
                    &account_manager,
                    duration_index,
                    suspend_duration,
                    &status_sender,
                    &state_sender,
                )
                .await;
                let Err(err) = result else {
                    // Proceeding to the next iteration of the loop
                    continue;
                };
                let (retry_sender, retry_receiver) = oneshot::channel();
                let result = state_sender
                    .send(State::Error {
                        error: err,
                        retry_sender,
                    })
                    .await;
                if result.is_err() {
                    // Message channel is dead hence user won't know we have an error, so RIP
                    break;
                }
                // Wait until retry attempted or aborted
                if retry_receiver.await.is_err() {
                    // Retry channel is dead so RIP
                    break;
                };
            }
        })
        .into();
    }

    async fn run(
        user: &User,
        account_manager: &AccountManager,
        duration_index: usize,
        suspend_duration: Duration,
        status_sender: &watch::Sender<Option<SystemStatus>>,
        state_sender: &mpsc::Sender<State>,
    ) -> anyhow::Result<()> {
        macro_rules! send_msg {
            ( $msg:expr ) => {
                state_sender
                    .send($msg)
                    .await
                    .context("Message channel closed")?;
            };
        }

        send_msg!(State::CheckingStatus);
        let status = account_manager.status(user).await?;

        status_sender
            .send(status.system_status.into())
            .context("State channel closed")?;

        let SystemStatus { ip, connection } = status.system_status;

        if !connection.is_active() {
            send_msg!(State::Approving(ip));
            account_manager.approve(user, duration_index, false).await?;
        } else {
            let (wake_sender, wake_receiver) = oneshot::channel();
            send_msg!(State::Suspended {
                duration: suspend_duration,
                wake_sender,
            });
            select! {
                _ = time::sleep(suspend_duration) => {}
                _ = wake_receiver => {}
            }
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}
