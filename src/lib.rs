use std::{
    io,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use bincode::Options;
use futures::{
    io::{AsyncRead, AsyncWrite},
    stream::Stream,
    Sink,
};
use interprocess::local_socket::{
    tokio::{LocalSocketStream, OwnedReadHalf, OwnedWriteHalf},
    ToLocalSocketName,
};
use serde::{de::DeserializeOwned, Serialize};

const U16_MARKER: u8 = 252;
const U32_MARKER: u8 = 253;
const U64_MARKER: u8 = 254;

#[derive(Debug)]
pub struct LocalSocketStreamTyped<T> {
    read: OwnedReadHalfTyped<T>,
    write: OwnedWriteHalfTyped<T>,
}

impl<T> LocalSocketStreamTyped<T> {
    pub async fn connect<'a>(name: impl ToLocalSocketName<'a>) -> io::Result<Self> {
        LocalSocketStream::connect(name).await.map(|raw| {
            let (read, write) = raw.into_split();
            Self {
                read: OwnedReadHalfTyped::new(read),
                write: OwnedWriteHalfTyped::new(write),
            }
        })
    }

    pub fn into_split(self) -> (OwnedReadHalfTyped<T>, OwnedWriteHalfTyped<T>) {
        (self.read, self.write)
    }

    pub fn peer_pid(&self) -> io::Result<u32> {
        self.read.peer_pid()
    }
}

impl<T: DeserializeOwned + Unpin> Stream for LocalSocketStreamTyped<T> {
    type Item = Result<T, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Stream::poll_next(Pin::new(&mut self.read), cx)
    }
}

impl<T: Serialize + Unpin> Sink<T> for LocalSocketStreamTyped<T> {
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

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error {0}")]
    Io(io::Error),
    #[error("bincode serialization/deserialization error {0}")]
    Bincode(bincode::Error),
}

fn bincode_options() -> impl Options {
    // Two of these are defaults, so you might say this is over specified. I say it's future proof, as
    // bincode default changes won't introduce accidental breaking changes.
    bincode::DefaultOptions::new()
        .with_limit(1024_u64.pow(2))
        .with_little_endian()
        .with_fixint_encoding()
        .reject_trailing_bytes()
}

#[derive(Debug)]
pub struct OwnedReadHalfTyped<T> {
    raw: OwnedReadHalf,
    current_item_len: Option<u64>,
    current_item_buffer: Vec<u8>,
    len_read_mode: Option<LenReadMode>,
    len_in_progress: [u8; 8],
    len_in_progress_assigned: u8,
    len_read: u64,
    _phantom: PhantomData<T>,
}

impl<T: DeserializeOwned + Unpin> Stream for OwnedReadHalfTyped<T> {
    type Item = Result<T, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let Self {
            ref mut raw,
            ref mut current_item_len,
            ref mut current_item_buffer,
            len_read_mode,
            len_in_progress,
            len_in_progress_assigned,
            len_read,
            _phantom,
        } = &mut *self;
        if let Some(current_item_len_inner) = current_item_len {
            while *len_read < *current_item_len_inner {
                match Pin::new(&mut *raw).poll_read(cx, current_item_buffer) {
                    Poll::Ready(Ok(len)) => {
                        if len == 0 {
                            return Poll::Ready(None);
                        }
                        *len_read += len as u64;
                        if *len_read == *current_item_len_inner {
                            *len_read = 0;
                            *current_item_len = None;
                            break;
                        }
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(Error::Io(e)))),
                    Poll::Pending => return Poll::Pending,
                }
            }

            Poll::Ready(Some(
                bincode_options()
                    .deserialize(current_item_buffer)
                    .map_err(Error::Bincode),
            ))
        } else if let Some(len_read_mode_inner) = len_read_mode {
            loop {
                let mut buf = [0; 8];
                let accumulated = *len_in_progress_assigned as usize;
                let slice = match len_read_mode_inner {
                    LenReadMode::U16 => &mut buf[0..(2 - accumulated)],
                    LenReadMode::U32 => &mut buf[0..(4 - accumulated)],
                    LenReadMode::U64 => &mut buf[0..(8 - accumulated)],
                };
                match Pin::new(&mut *raw).poll_read(cx, slice) {
                    Poll::Ready(Ok(len)) => {
                        if len == 0 {
                            return Poll::Ready(None);
                        }
                        len_in_progress[accumulated..].copy_from_slice(slice);
                        *len_in_progress_assigned += len as u8;
                        if len == slice.len() {
                            *current_item_len = Some(match len_read_mode_inner {
                                LenReadMode::U16 => u16::from_le_bytes(
                                    (&len_in_progress[0..2]).try_into().expect("infallible"),
                                ) as u64,
                                LenReadMode::U32 => u32::from_le_bytes(
                                    (&len_in_progress[0..4]).try_into().expect("infallible"),
                                ) as u64,
                                LenReadMode::U64 => u64::from_le_bytes(*len_in_progress),
                            });
                            *len_read_mode = None;
                            *len_in_progress_assigned = 0;
                            return self.poll_next(cx);
                        }
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(Error::Io(e)))),
                    Poll::Pending => return Poll::Pending,
                }
            }
        } else {
            let mut buf = [0];
            match Pin::new(&mut self.raw).poll_read(cx, &mut buf) {
                Poll::Ready(Ok(len)) => {
                    if len == 0 {
                        Poll::Ready(None)
                    } else {
                        match buf[0] {
                            U16_MARKER => {
                                self.len_read_mode = Some(LenReadMode::U16);
                            }
                            U32_MARKER => {
                                self.len_read_mode = Some(LenReadMode::U32);
                            }
                            U64_MARKER => {
                                self.len_read_mode = Some(LenReadMode::U64);
                            }
                            other => {
                                self.current_item_len = Some(other as u64);
                            }
                        }
                        self.poll_next(cx)
                    }
                }
                Poll::Ready(Err(e)) => Poll::Ready(Some(Err(Error::Io(e)))),
                Poll::Pending => Poll::Pending,
            }
        }
    }
}

impl<T> OwnedReadHalfTyped<T> {
    fn new(raw: OwnedReadHalf) -> Self {
        Self {
            raw,
            current_item_len: None,
            current_item_buffer: Vec::new(),
            len_read_mode: None,
            len_in_progress: Default::default(),
            len_in_progress_assigned: 0,
            len_read: 0,
            _phantom: PhantomData,
        }
    }

    pub fn peer_pid(&self) -> io::Result<u32> {
        self.raw.peer_pid()
    }
}

#[derive(Debug)]
enum LenReadMode {
    U16,
    U32,
    U64,
}

#[derive(Debug)]
pub struct OwnedWriteHalfTyped<T> {
    raw: OwnedWriteHalf,
    primed_value: Option<T>,
    bytes_being_sent: Vec<u8>,
    bytes_sent: usize,
    current_len: [u8; 9],
    len_to_be_sent: u8,
}

impl<T: Serialize + Unpin> Sink<T> for OwnedWriteHalfTyped<T> {
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.raw)
            .poll_write(cx, &[])
            .map(|r| r.map(|_| ()).map_err(Error::Io))
    }

    fn start_send(mut self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        self.primed_value = Some(item);
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.as_mut().maybe_send(cx) {
            Poll::Ready(Ok(Some(()))) => {
                // Send successful, poll_flush now
                Pin::new(&mut self.raw)
                    .poll_flush(cx)
                    .map(|r| r.map_err(Error::Io))
            }
            Poll::Ready(Ok(None)) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.as_mut().maybe_send(cx) {
            Poll::Ready(Ok(Some(()))) => {
                // Send successful, poll_close now
                Pin::new(&mut self.raw)
                    .poll_close(cx)
                    .map(|r| r.map_err(Error::Io))
            }
            Poll::Ready(Ok(None)) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T: Serialize + Unpin> OwnedWriteHalfTyped<T> {
    fn maybe_send(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<()>, Error>> {
        let Self {
            raw,
            primed_value,
            bytes_being_sent,
            bytes_sent,
            current_len,
            len_to_be_sent,
        } = &mut *self;
        if let Some(item) = primed_value.take() {
            let to_send = bincode_options().serialize(&item).map_err(Error::Bincode)?;
            let (new_current_len, to_be_sent) = if to_send.len() < U16_MARKER as usize {
                let bytes = (to_send.len() as u8).to_le_bytes();
                ([bytes[0], 0, 0, 0, 0, 0, 0, 0, 0], 1)
            } else if (to_send.len() as u64) < 2_u64.pow(16) {
                let bytes = (to_send.len() as u16).to_le_bytes();
                ([U16_MARKER, bytes[0], bytes[1], 0, 0, 0, 0, 0, 0], 3)
            } else if (to_send.len() as u64) < 2_u64.pow(32) {
                let bytes = (to_send.len() as u32).to_le_bytes();
                (
                    [
                        U32_MARKER, bytes[0], bytes[1], bytes[2], bytes[3], 0, 0, 0, 0,
                    ],
                    5,
                )
            } else {
                let bytes = (to_send.len() as u64).to_le_bytes();
                (
                    [
                        U64_MARKER, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5],
                        bytes[6], bytes[7],
                    ],
                    9,
                )
            };
            *bytes_sent = 0;
            match Pin::new(&mut *raw).poll_write(cx, &current_len[0..to_be_sent]) {
                Poll::Ready(Ok(len)) => {
                    if len == 0 {
                        Poll::Ready(Ok(None))
                    } else if len == to_be_sent {
                        *bytes_being_sent = to_send;
                        *len_to_be_sent = 0;
                        self.maybe_send(cx)
                    } else {
                        *bytes_being_sent = to_send;
                        *current_len = new_current_len;
                        *len_to_be_sent = (to_be_sent - len) as u8;
                        self.maybe_send(cx)
                    }
                }
                Poll::Ready(Err(e)) => Poll::Ready(Err(Error::Io(e))),
                Poll::Pending => {
                    *current_len = new_current_len;
                    *len_to_be_sent = to_be_sent as u8;
                    *bytes_being_sent = to_send;
                    Poll::Pending
                }
            }
        } else if *len_to_be_sent > 0 {
            match Pin::new(&mut *raw).poll_write(cx, &current_len[0..(*len_to_be_sent as usize)]) {
                Poll::Ready(Ok(len)) => {
                    if len == 0 {
                        Poll::Ready(Ok(None))
                    } else if len == *len_to_be_sent as usize {
                        *len_to_be_sent = 0;
                        self.maybe_send(cx)
                    } else {
                        *len_to_be_sent -= len as u8;
                        self.maybe_send(cx)
                    }
                }
                Poll::Ready(Err(e)) => Poll::Ready(Err(Error::Io(e))),
                Poll::Pending => Poll::Pending,
            }
        } else {
            loop {
                if *bytes_sent < bytes_being_sent.len() {
                    match Pin::new(&mut *raw).poll_write(cx, &bytes_being_sent[*bytes_sent..]) {
                        Poll::Ready(Ok(len)) => {
                            if len == 0 {
                                return Poll::Ready(Ok(None));
                            } else {
                                *bytes_sent += len;
                                if *bytes_sent == bytes_being_sent.len() {
                                    *bytes_sent = 0;
                                    bytes_being_sent.clear();
                                    return Poll::Ready(Ok(Some(())));
                                }
                            }
                        }
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(Error::Io(e))),
                        Poll::Pending => return Poll::Pending,
                    }
                } else {
                    return Poll::Ready(Ok(Some(())));
                }
            }
        }
    }
}

impl<T> OwnedWriteHalfTyped<T> {
    fn new(raw: OwnedWriteHalf) -> Self {
        Self {
            raw,
            primed_value: None,
            bytes_being_sent: Vec::new(),
            bytes_sent: 0,
            current_len: Default::default(),
            len_to_be_sent: 0,
        }
    }

    pub fn peer_pid(&self) -> io::Result<u32> {
        self.raw.peer_pid()
    }
}
