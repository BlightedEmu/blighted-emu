// SPDX-License-Identifier: AGPL-3.0-or-later
//! A typed wrapper of the NATS client.

use std::marker::PhantomData;
use std::time::Duration;

use async_nats::{Client, Subject, Subscriber};
use bytes::Bytes;
use futures_util::StreamExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::message::{decode, encode};

/// A bus failure.
#[derive(Debug, Error)]
pub enum BusError {
    /// NATS cannot be reached.
    #[error("NATS connect failed: {0}")]
    Connect(#[from] async_nats::ConnectError),
    /// A publish failed.
    #[error("NATS publish failed: {0}")]
    Publish(#[from] async_nats::PublishError),
    /// A subscribe failed.
    #[error("NATS subscribe failed: {0}")]
    Subscribe(#[from] async_nats::SubscribeError),
    /// A request failed, had no responder, or timed out.
    #[error("NATS request failed: {0}")]
    Request(#[from] async_nats::RequestError),
    /// A message cannot be encoded.
    #[error("bus message encode failed: {0}")]
    Encode(postcard::Error),
    /// A message is not the expected type.
    #[error("bus message decode failed: {0}")]
    Decode(postcard::Error),
    /// Waiting messages could not be sent.
    #[error("NATS flush failed: {0}")]
    Flush(#[from] async_nats::client::FlushError),
    /// A request has no reply subject.
    #[error("bus message has no reply subject")]
    NoReplySubject,
}

/// A connection to NATS.
#[derive(Debug, Clone)]
pub struct Bus {
    client: Client,
}

/// One received bus message and its reply subject, when it is a request.
#[derive(Debug)]
pub struct Incoming<T> {
    /// The message.
    pub message: T,
    reply: Option<Subject>,
}

/// A typed subscription.
#[derive(Debug)]
pub struct Subscription<T> {
    subscriber: Subscriber,
    message: PhantomData<T>,
}

impl Bus {
    /// Connects to `url`, for example `nats://127.0.0.1:4222`.
    ///
    /// # Errors
    ///
    /// Returns [`BusError::Connect`] when NATS cannot be reached.
    pub async fn connect(url: &str) -> Result<Self, BusError> {
        Ok(Self {
            client: async_nats::connect(url).await?,
        })
    }

    /// Publishes `message` on `subject`.
    ///
    /// # Errors
    ///
    /// Returns [`BusError`] when encoding or publishing fails.
    pub async fn publish<T: Serialize>(
        &self,
        subject: String,
        message: &T,
    ) -> Result<(), BusError> {
        self.client
            .publish(subject, Bytes::from(encode(message)?))
            .await?;
        Ok(())
    }

    /// Sends `message` as a request and waits at most `timeout` for the reply.
    ///
    /// # Errors
    ///
    /// Returns [`BusError::Request`] on a timeout or with no responder, and [`BusError::Decode`]
    /// for a reply of another type.
    pub async fn request<T: Serialize, R: DeserializeOwned>(
        &self,
        subject: String,
        message: &T,
        timeout: Duration,
    ) -> Result<R, BusError> {
        let request = async_nats::Request::new()
            .payload(Bytes::from(encode(message)?))
            .timeout(Some(timeout));
        let reply = self.client.send_request(subject, request).await?;
        decode(&reply.payload)
    }

    /// Answers a request.
    ///
    /// # Errors
    ///
    /// Returns [`BusError::NoReplySubject`] when `request` was not sent as a request.
    pub async fn reply<T, R: Serialize>(
        &self,
        request: &Incoming<T>,
        answer: &R,
    ) -> Result<(), BusError> {
        let subject = request.reply.clone().ok_or(BusError::NoReplySubject)?;
        self.client
            .publish(subject, Bytes::from(encode(answer)?))
            .await?;
        Ok(())
    }

    /// Subscribes to `subject`; the messages must be of type `T`.
    ///
    /// # Errors
    ///
    /// Returns [`BusError::Subscribe`] when NATS refuses the subscription.
    pub async fn subscribe<T: DeserializeOwned>(
        &self,
        subject: String,
    ) -> Result<Subscription<T>, BusError> {
        Ok(Subscription {
            subscriber: self.client.subscribe(subject).await?,
            message: PhantomData,
        })
    }

    /// Sends everything that waits in the client.
    ///
    /// # Errors
    ///
    /// Returns [`BusError::Flush`] when NATS is gone.
    pub async fn flush(&self) -> Result<(), BusError> {
        Ok(self.client.flush().await?)
    }
}

impl<T: DeserializeOwned> Subscription<T> {
    /// The next message; `None` when the subscription ended. A message of another type is an
    /// error and does not end the subscription.
    pub async fn next(&mut self) -> Option<Result<Incoming<T>, BusError>> {
        let message = self.subscriber.next().await?;
        Some(decode(&message.payload).map(|decoded| Incoming {
            message: decoded,
            reply: message.reply,
        }))
    }
}

#[cfg(all(test, feature = "infra"))]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::message::{AdmitReply, Heartbeat};
    use crate::test_support::test_bus;

    const TIMEOUT: Duration = Duration::from_secs(2);
    const STARTED_AT_MS: u64 = 5;

    #[tokio::test]
    async fn publish_subscribe_and_request_reply_work() {
        let bus = test_bus().await;
        let subject = format!("nw.test.{}", Uuid::new_v4());
        let mut subscription = bus
            .subscribe::<Heartbeat>(subject.clone())
            .await
            .expect("sub");
        bus.flush().await.expect("flush");
        let beat = Heartbeat {
            id: crate::id::BusId::try_from("gw-test".to_owned()).expect("id"),
            started_at_ms: STARTED_AT_MS,
        };
        bus.publish(subject.clone(), &beat).await.expect("publish");
        let incoming = subscription.next().await.expect("one").expect("decode");
        assert_eq!(incoming.message, beat);

        let request_subject = format!("nw.test.{}", Uuid::new_v4());
        let mut requests = bus
            .subscribe::<Heartbeat>(request_subject.clone())
            .await
            .expect("sub");
        bus.flush().await.expect("flush");
        let answerer = bus.clone();
        tokio::spawn(async move {
            let request = requests.next().await.expect("one").expect("decode");
            answerer
                .reply(&request, &AdmitReply::Admitted)
                .await
                .expect("reply");
        });
        let reply: AdmitReply = bus
            .request(request_subject, &beat, TIMEOUT)
            .await
            .expect("request");
        assert_eq!(reply, AdmitReply::Admitted);
    }

    #[tokio::test]
    async fn a_request_without_a_responder_fails_fast() {
        let bus = test_bus().await;
        let subject = format!("nw.test.{}", Uuid::new_v4());
        let result: Result<AdmitReply, _> = bus
            .request(
                subject,
                &Heartbeat {
                    id: crate::id::BusId::try_from("gw-test".to_owned()).expect("id"),
                    started_at_ms: STARTED_AT_MS,
                },
                TIMEOUT,
            )
            .await;
        assert!(matches!(result, Err(BusError::Request(_))));
    }
}
