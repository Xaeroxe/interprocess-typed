// Copyright 2022 Jacob Kiesel
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![doc = include_str!("../README.md")]

use std::{
    ffi::OsString,
    io,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use async_io_typed::{AsyncReadTyped, AsyncWriteTyped};
use futures_util::{stream::Stream, Sink};
use interprocess::local_socket::tokio::{
    LocalSocketListener, LocalSocketStream, OwnedReadHalf, OwnedWriteHalf,
};
use serde::{de::DeserializeOwned, Serialize};

#[cfg(test)]
mod tests;

pub use interprocess::local_socket::ToLocalSocketName;

/// Used by a server to accept new connections.
#[derive(Debug)]
pub struct LocalSocketListenerTyped<T: Serialize + DeserializeOwned + Unpin> {
    raw: LocalSocketListener,
    _phantom: PhantomData<T>,
}

impl<T: Serialize + DeserializeOwned + Unpin> LocalSocketListenerTyped<T> {
    /// Begins listening for connections to the given socket name. The socket does not need to exist
    /// prior to calling this function.
    pub fn bind<'a>(name: impl ToLocalSocketName<'a>) -> io::Result<Self> {
        LocalSocketListener::bind(name).map(|raw| LocalSocketListenerTyped {
            raw,
            _phantom: PhantomData,
        })
    }

    /// Accepts the connection, initializing it with the given size limit specified in bytes.
    ///
    /// Be careful, large limits might create a vulnerability to a Denial of Service attack.
    pub async fn accept_with_limit(
        &self,
        size_limit: u64,
    ) -> io::Result<LocalSocketStreamTyped<T>> {
        self.raw.accept().await.map(|raw| {
            let (read, write) = raw.into_split();
            LocalSocketStreamTyped {
                read: OwnedReadHalfTyped::<T>::new(read, size_limit),
                write: OwnedWriteHalfTyped::<T>::new(write, size_limit),
            }
        })
    }

    /// Accepts the connection, initializing it with a default size limit of 1 MB per message.
    pub async fn accept(&self) -> io::Result<LocalSocketStreamTyped<T>> {
        self.accept_with_limit(1024_u64.pow(2)).await
    }
}

/// A duplex async connection for sending and receiving messages of a particular type.
#[derive(Debug)]
pub struct LocalSocketStreamTyped<T: Serialize + DeserializeOwned + Unpin> {
    read: OwnedReadHalfTyped<T>,
    write: OwnedWriteHalfTyped<T>,
}

impl<T: Serialize + DeserializeOwned + Unpin> LocalSocketStreamTyped<T> {
    /// Creates a connection, initializing it with the given size limit specified in bytes.
    ///
    /// Be careful, large limits might create a vulnerability to a Denial of Service attack.
    pub async fn connect_with_limit<'a>(
        name: impl ToLocalSocketName<'a>,
        size_limit: u64,
    ) -> io::Result<Self> {
        LocalSocketStream::connect(name).await.map(|raw| {
            let (read, write) = raw.into_split();
            Self {
                read: OwnedReadHalfTyped::new(read, size_limit),
                write: OwnedWriteHalfTyped::new(write, size_limit),
            }
        })
    }

    /// Creates a connection, initializing it with a default size limit of 1 MB per message.
    pub async fn connect<'a>(name: impl ToLocalSocketName<'a>) -> io::Result<Self> {
        Self::connect_with_limit(name, 1024_u64.pow(2)).await
    }

    /// Splits this into two parts, the first can be used for reading from the socket,
    /// the second can be used for writing to the socket.
    pub fn into_split(self) -> (OwnedReadHalfTyped<T>, OwnedWriteHalfTyped<T>) {
        (self.read, self.write)
    }

    /// Returns the process id of the connected peer.
    pub fn peer_pid(&self) -> io::Result<u32> {
        self.read.peer_pid()
    }
}

impl<T: Serialize + DeserializeOwned + Unpin> Stream for LocalSocketStreamTyped<T> {
    type Item = Result<T, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Stream::poll_next(Pin::new(&mut self.read), cx)
    }
}

impl<T: Serialize + DeserializeOwned + Unpin> Sink<T> for LocalSocketStreamTyped<T> {
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::poll_ready(Pin::new(&mut self.write), cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        Sink::start_send(Pin::new(&mut self.write), item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::poll_flush(Pin::new(&mut self.write), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::poll_close(Pin::new(&mut self.write), cx)
    }
}

/// Errors that might arise while using a typed local socket connection.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error from `std::io`
    #[error("io error {0}")]
    Io(io::Error),
    /// Error from the `bincode` crate
    #[error("bincode serialization/deserialization error {0}")]
    Bincode(bincode::Error),
    /// A message was sent that exceeded the configured length limit
    #[error("message sent exceeded configured length limit")]
    SentMessageTooLarge,
    /// A message was received that exceeded the configured length limit
    #[error("message received exceeded configured length limit, terminating connection")]
    ReceivedMessageTooLarge,
}

impl From<async_io_typed::Error> for Error {
    fn from(e: async_io_typed::Error) -> Self {
        match e {
            async_io_typed::Error::Io(e) => Self::Io(e),
            async_io_typed::Error::Bincode(e) => Self::Bincode(e),
            async_io_typed::Error::SentMessageTooLarge => Self::SentMessageTooLarge,
            async_io_typed::Error::ReceivedMessageTooLarge => Self::ReceivedMessageTooLarge,
        }
    }
}

/// The read half of a [LocalSocketStreamTyped], generated by [LocalSocketStreamTyped::into_split]
#[derive(Debug)]
pub struct OwnedReadHalfTyped<T: Serialize + DeserializeOwned + Unpin> {
    raw: AsyncReadTyped<OwnedReadHalf, T>,
}

impl<T: Serialize + DeserializeOwned + Unpin> Stream for OwnedReadHalfTyped<T> {
    type Item = Result<T, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.raw).poll_next(cx).map_err(Into::into)
    }
}

impl<T: Serialize + DeserializeOwned + Unpin> OwnedReadHalfTyped<T> {
    fn new(raw: OwnedReadHalf, size_limit: u64) -> Self {
        Self {
            raw: AsyncReadTyped::new_with_limit(raw, size_limit),
        }
    }

    /// Returns the process id of the connected peer.
    pub fn peer_pid(&self) -> io::Result<u32> {
        self.raw.inner().peer_pid()
    }
}

/// The write half of a [LocalSocketStreamTyped], generated by [LocalSocketStreamTyped::into_split]
#[derive(Debug)]
pub struct OwnedWriteHalfTyped<T: Serialize + DeserializeOwned + Unpin> {
    raw: AsyncWriteTyped<OwnedWriteHalf, T>,
}

impl<T: Serialize + DeserializeOwned + Unpin> Sink<T> for OwnedWriteHalfTyped<T> {
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.raw).poll_ready(cx).map_err(Into::into)
    }

    fn start_send(mut self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        Pin::new(&mut self.raw).start_send(item).map_err(Into::into)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.raw).poll_flush(cx).map_err(Into::into)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.raw).poll_close(cx).map_err(Into::into)
    }
}

impl<T: Serialize + DeserializeOwned + Unpin> OwnedWriteHalfTyped<T> {
    fn new(raw: OwnedWriteHalf, size_limit: u64) -> Self {
        Self {
            raw: AsyncWriteTyped::new_with_limit(raw, size_limit),
        }
    }

    /// Returns the process id of the connected peer.
    pub fn peer_pid(&self) -> io::Result<u32> {
        self.raw.inner().peer_pid()
    }
}

/// Randomly generates a socket name suitable for the operating system in use.
pub fn generate_socket_name() -> io::Result<OsString> {
    #[cfg(windows)]
    {
        Ok(OsString::from(format!("@{}.sock", uuid::Uuid::new_v4())))
    }
    #[cfg(not(windows))]
    {
        let path = tempfile::tempdir()?.into_path().join("ipc_socket");
        Ok(path.into_os_string())
    }
}
