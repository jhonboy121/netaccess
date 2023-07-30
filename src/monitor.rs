use crate::{
    account_manager::{AccountManager, Connection},
    user::User,
};
use anyhow::Context;
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio::{
    select,
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time,
};

#[derive(Debug)]
pub enum Message {
    Suspended {
        duration: Duration,
        wake_sender: oneshot::Sender<()>,
    },
    CheckingStatus,
    Status {
        ip: IpAddr,
        connection: Connection,
    },
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
        message_sender: mpsc::Sender<Message>,
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
                    &message_sender,
                )
                .await;
                let Err(err) = result else {
                    // Proceeding to the next iteration of the loop
                    continue;
                };
                let (retry_sender, retry_receiver) = oneshot::channel();
                let result = message_sender
                    .send(Message::Error {
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
        message_sender: &mpsc::Sender<Message>,
    ) -> anyhow::Result<()> {
        let send_msg = |msg| async {
            message_sender
                .send(msg)
                .await
                .context("Message channel closed")
        };

        send_msg(Message::CheckingStatus).await?;
        let status = account_manager.status(user).await?;

        let (ip, connection) = status.system_connection();
        send_msg(Message::Status {
            ip: *ip,
            connection: connection.clone(),
        })
        .await?;

        if !connection.is_active() {
            send_msg(Message::Approving(*ip)).await?;
            account_manager
                .approve(user, duration_index, false)
                .await?;
        } else {
            let (wake_sender, wake_receiver) = oneshot::channel();
            send_msg(Message::Suspended {
                duration: suspend_duration,
                wake_sender,
            })
            .await?;
            select! {
                _ = time::sleep(suspend_duration) => {}
                _ = wake_receiver => {}
            }
        }
        Ok(())
    }
}
