use std::{
    ffi::OsString,
    io,
    marker::PhantomData,
    mem::{self, ManuallyDrop},
    pin::Pin,
    task::{Context, Poll}, collections::VecDeque,
};

use bincode::Options;
use futures::{
    io::{AsyncRead, AsyncWrite},
    stream::Stream,
    AsyncWriteExt, Sink,
};
use interprocess::local_socket::{
    tokio::{LocalSocketListener, LocalSocketStream, OwnedReadHalf, OwnedWriteHalf},
    ToLocalSocketName,
};
use serde::{de::DeserializeOwned, Serialize};

#[cfg(test)]
mod tests;

const U16_MARKER: u8 = 252;
const U32_MARKER: u8 = 253;
const U64_MARKER: u8 = 254;
const ZST_MARKER: u8 = 255;

#[derive(Debug)]
pub struct LocalSocketListenerTyped<T> {
    raw: LocalSocketListener,
    _phantom: PhantomData<T>,
}

impl<T> LocalSocketListenerTyped<T> {
    pub fn bind<'a>(name: impl ToLocalSocketName<'a>) -> io::Result<Self> {
        LocalSocketListener::bind(name).map(|raw| LocalSocketListenerTyped {
            raw,
            _phantom: PhantomData,
        })
    }

    pub async fn accept(&self) -> io::Result<LocalSocketStreamTyped<T>> {
        self.raw.accept().await.map(|raw| {
            let (read, write) = raw.into_split();
            LocalSocketStreamTyped {
                read: OwnedReadHalfTyped::<T>::new(read),
                write: OwnedWriteHalfTyped::<T>::new(write),
            }
        })
    }
}

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
    state: ReadHalfState,
    _phantom: PhantomData<T>,
}

#[derive(Debug)]
enum ReadHalfState {
    Idle,
    ReadingLen {
        len_read_mode: LenReadMode,
        len_in_progress: [u8; 8],
        len_in_progress_assigned: u8,
    },
    ReadingItem {
        current_item_len: usize,
        len_read: usize,
        current_item_buffer: Box<[u8]>,
    },
    Finished,
}

impl<T: DeserializeOwned + Unpin> Stream for OwnedReadHalfTyped<T> {
    type Item = Result<T, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let Self {
            ref mut raw,
            ref mut state,
            _phantom,
        } = &mut *self;
        match state {
            ReadHalfState::Idle => {
                let mut buf = [0];
                match Pin::new(raw).poll_read(cx, &mut buf) {
                    Poll::Ready(Ok(_len)) => {
                        match buf[0] {
                            U16_MARKER => {
                                *state = ReadHalfState::ReadingLen {
                                    len_read_mode: LenReadMode::U16,
                                    len_in_progress: Default::default(),
                                    len_in_progress_assigned: 0,
                                };
                            }
                            U32_MARKER => {
                                *state = ReadHalfState::ReadingLen {
                                    len_read_mode: LenReadMode::U32,
                                    len_in_progress: Default::default(),
                                    len_in_progress_assigned: 0,
                                };
                            }
                            U64_MARKER => {
                                *state = ReadHalfState::ReadingLen {
                                    len_read_mode: LenReadMode::U64,
                                    len_in_progress: Default::default(),
                                    len_in_progress_assigned: 0,
                                };
                            }
                            ZST_MARKER => {
                                return Poll::Ready(Some(
                                    bincode_options().deserialize(&[]).map_err(Error::Bincode),
                                ));
                            }
                            0 => {
                                *state = ReadHalfState::Finished;
                                return Poll::Ready(None);
                            }
                            other => {
                                *state = ReadHalfState::ReadingItem {
                                    current_item_len: other as usize,
                                    current_item_buffer: vec![0; other as usize].into_boxed_slice(),
                                    len_read: 0,
                                };
                            }
                        }
                        self.poll_next(cx)
                    }
                    Poll::Ready(Err(e)) => Poll::Ready(Some(Err(Error::Io(e)))),
                    Poll::Pending => Poll::Pending,
                }
            }
            ReadHalfState::ReadingLen {
                ref mut len_read_mode,
                ref mut len_in_progress,
                ref mut len_in_progress_assigned,
            } => loop {
                let mut buf = [0; 8];
                let accumulated = *len_in_progress_assigned as usize;
                let slice = match len_read_mode {
                    LenReadMode::U16 => &mut buf[0..(2 - accumulated)],
                    LenReadMode::U32 => &mut buf[0..(4 - accumulated)],
                    LenReadMode::U64 => &mut buf[0..(8 - accumulated)],
                };
                match Pin::new(&mut *raw).poll_read(cx, slice) {
                    Poll::Ready(Ok(len)) => {
                        len_in_progress[accumulated..(accumulated + slice.len())]
                            .copy_from_slice(&slice[..len]);
                        *len_in_progress_assigned += len as u8;
                        if len == slice.len() {
                            let new_len = match len_read_mode {
                                LenReadMode::U16 => u16::from_le_bytes(
                                    (&len_in_progress[0..2]).try_into().expect("infallible"),
                                ) as u64,
                                LenReadMode::U32 => u32::from_le_bytes(
                                    (&len_in_progress[0..4]).try_into().expect("infallible"),
                                ) as u64,
                                LenReadMode::U64 => u64::from_le_bytes(*len_in_progress),
                            };
                            *state = ReadHalfState::ReadingItem {
                                len_read: 0,
                                current_item_len: new_len as usize,
                                current_item_buffer: vec![0; new_len as usize].into_boxed_slice(),
                            };
                            return self.poll_next(cx);
                        }
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(Error::Io(e)))),
                    Poll::Pending => return Poll::Pending,
                }
            },
            ReadHalfState::ReadingItem {
                ref mut len_read,
                ref mut current_item_len,
                ref mut current_item_buffer,
            } => {
                while *len_read < *current_item_len {
                    match Pin::new(&mut *raw).poll_read(cx, &mut current_item_buffer[*len_read..]) {
                        Poll::Ready(Ok(len)) => {
                            *len_read += len;
                            if *len_read == *current_item_len {
                                break;
                            }
                        }
                        Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(Error::Io(e)))),
                        Poll::Pending => return Poll::Pending,
                    }
                }
                let ret = Poll::Ready(Some(
                    bincode_options()
                        .deserialize(current_item_buffer)
                        .map_err(Error::Bincode),
                ));
                *state = ReadHalfState::Idle;
                ret
            }
            ReadHalfState::Finished => Poll::Ready(None),
        }
    }
}

impl<T> OwnedReadHalfTyped<T> {
    fn new(raw: OwnedReadHalf) -> Self {
        Self {
            raw,
            state: ReadHalfState::Idle,
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
    raw: ManuallyDrop<OwnedWriteHalf>,
    state: WriteHalfState,
    primed_values: VecDeque<T>,
}

#[derive(Debug)]
enum WriteHalfState {
    Idle,
    WritingLen {
        bytes_being_sent: Vec<u8>,
        current_len: [u8; 9],
        len_to_be_sent: u8,
    },
    WritingValue {
        bytes_being_sent: Vec<u8>,
        bytes_sent: usize,
    },
}

impl<T: Serialize + Unpin> Sink<T> for OwnedWriteHalfTyped<T> {
    type Error = Error;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(mut self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        self.primed_values.push_front(item);
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.as_mut().maybe_send(cx) {
            Poll::Ready(Ok(Some(()))) => {
                // Send successful, poll_flush now
                Pin::new(&mut *self.raw)
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
                Pin::new(&mut *self.raw)
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
            ref mut raw,
            ref mut state,
            ref mut primed_values,
        } = &mut *self;
        match state {
            WriteHalfState::Idle => {
                if let Some(item) = primed_values.pop_back() {
                    let to_send = bincode_options().serialize(&item).map_err(Error::Bincode)?;
                    let (new_current_len, to_be_sent) = if to_send.is_empty() {
                        ([ZST_MARKER, 0, 0, 0, 0, 0, 0, 0, 0], 1)
                    } else if to_send.len() < U16_MARKER as usize {
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
                                U64_MARKER, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4],
                                bytes[5], bytes[6], bytes[7],
                            ],
                            9,
                        )
                    };
                    match Pin::new(&mut **raw).poll_write(cx, &new_current_len[0..to_be_sent]) {
                        Poll::Ready(Ok(len)) => {
                            if len == to_be_sent {
                                *state = WriteHalfState::WritingValue {
                                    bytes_being_sent: to_send,
                                    bytes_sent: 0,
                                };
                                self.maybe_send(cx)
                            } else {
                                *state = WriteHalfState::WritingLen {
                                    current_len: new_current_len,
                                    len_to_be_sent: (to_be_sent - len) as u8,
                                    bytes_being_sent: to_send,
                                };
                                self.maybe_send(cx)
                            }
                        }
                        Poll::Ready(Err(e)) => Poll::Ready(Err(Error::Io(e))),
                        Poll::Pending => {
                            *state = WriteHalfState::WritingLen {
                                current_len: new_current_len,
                                len_to_be_sent: to_be_sent as u8,
                                bytes_being_sent: to_send,
                            };
                            Poll::Pending
                        }
                    }
                } else {
                    Poll::Ready(Ok(Some(())))
                }
            }
            WriteHalfState::WritingLen {
                current_len,
                len_to_be_sent,
                bytes_being_sent,
            } => {
                match Pin::new(&mut **raw)
                    .poll_write(cx, &current_len[0..(*len_to_be_sent as usize)])
                {
                    Poll::Ready(Ok(len)) => {
                        if len == *len_to_be_sent as usize {
                            *state = WriteHalfState::WritingValue {
                                bytes_being_sent: mem::take(bytes_being_sent),
                                bytes_sent: 0,
                            };
                            self.maybe_send(cx)
                        } else {
                            *len_to_be_sent -= len as u8;
                            self.maybe_send(cx)
                        }
                    }
                    Poll::Ready(Err(e)) => Poll::Ready(Err(Error::Io(e))),
                    Poll::Pending => Poll::Pending,
                }
            }
            WriteHalfState::WritingValue {
                bytes_being_sent,
                bytes_sent,
            } => loop {
                if *bytes_sent < bytes_being_sent.len() {
                    match Pin::new(&mut **raw).poll_write(cx, &bytes_being_sent[*bytes_sent..]) {
                        Poll::Ready(Ok(len)) => {
                            *bytes_sent += len;
                            if *bytes_sent == bytes_being_sent.len() {
                                *state = WriteHalfState::Idle;
                                return Poll::Ready(Ok(Some(())));
                            }
                        }
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(Error::Io(e))),
                        Poll::Pending => return Poll::Pending,
                    }
                } else {
                    *state = WriteHalfState::Idle;
                    return Poll::Ready(Ok(Some(())));
                }
            },
        }
    }
}

impl<T> OwnedWriteHalfTyped<T> {
    fn new(raw: OwnedWriteHalf) -> Self {
        Self {
            raw: ManuallyDrop::new(raw),
            state: WriteHalfState::Idle,
            primed_values: VecDeque::new(),
        }
    }

    pub fn peer_pid(&self) -> io::Result<u32> {
        self.raw.peer_pid()
    }
}

impl<T> Drop for OwnedWriteHalfTyped<T> {
    fn drop(&mut self) {
        let mut raw = unsafe { ManuallyDrop::take(&mut self.raw) };
        if let WriteHalfState::Idle { .. } = self.state {
            tokio::spawn(async move {
                let _ = raw.write_all(&[0]).await;
            });
        }
    }
}

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
