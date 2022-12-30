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

use std::mem;

use bincode::Options;
use futures_util::{SinkExt, StreamExt};
use tokio::task::JoinHandle;

use super::*;

fn start_send_helper<T: Serialize + DeserializeOwned + Unpin + Send + 'static>(
    mut s: LocalSocketStreamTyped<T>,
    value: T,
) -> JoinHandle<(LocalSocketStreamTyped<T>, Result<(), Error>)> {
    tokio::spawn(async move {
        let ret = s.send(value).await;
        (s, ret)
    })
}

#[tokio::test]
async fn hello_world() {
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u8>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u8>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    let message = "Hello, world!".as_bytes().to_vec();
    let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
    assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
    fut.await.unwrap().1.unwrap();
}

#[tokio::test]
async fn shutdown_after_hello_world() {
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u8>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u8>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    let message = "Hello, world!".as_bytes().to_vec();
    let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
    assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
    let (server_stream, result) = fut.await.unwrap();
    result.unwrap();
    mem::drop(server_stream);
    let next = client_stream.next().await;
    assert!(next.is_none(), "{next:?} was not none");
}

#[tokio::test]
async fn hello_worlds() {
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u8>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u8>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    for i in 0..100 {
        let message = format!("Hello, world {}!", i).into_bytes();
        let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
        assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
        let (stream, result) = fut.await.unwrap();
        server_stream = Some(stream);
        result.unwrap();
    }
}

#[tokio::test]
async fn zero_len_message() {
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<()>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<()>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    let fut = start_send_helper(server_stream.take().unwrap(), ());
    client_stream.next().await.unwrap().unwrap();
    fut.await.unwrap().1.unwrap();
}

#[tokio::test]
async fn zero_len_messages() {
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<()>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<()>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    for _ in 0..100 {
        let fut = start_send_helper(server_stream.take().unwrap(), ());
        client_stream.next().await.unwrap().unwrap();
        let (stream, result) = fut.await.unwrap();
        server_stream = Some(stream);
        result.unwrap();
    }
}

#[tokio::test]
async fn u16_len_message() {
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u16>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u16>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    let message = (0..(u8::MAX as u16 + 1) / 2).collect::<Vec<_>>();
    let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
    assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
    fut.await.unwrap().1.unwrap();
}

#[tokio::test]
async fn u16_len_messages() {
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u16>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u16>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    for _ in 0..10 {
        let message = (0..(u8::MAX as u16 + 1) / 2).collect::<Vec<_>>();
        let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
        assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
        let (stream, result) = fut.await.unwrap();
        server_stream = Some(stream);
        result.unwrap();
    }
}

#[tokio::test]
async fn u32_len_message() {
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u32>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u32>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    let message = (0..(u16::MAX as u32 + 1) / 4).collect::<Vec<_>>();
    let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
    assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
    fut.await.unwrap().1.unwrap();
}

#[tokio::test]
async fn u32_len_messages() {
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u32>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u32>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    for _ in 0..10 {
        let message = (0..(u16::MAX as u32 + 1) / 4).collect::<Vec<_>>();
        let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
        assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
        let (stream, result) = fut.await.unwrap();
        server_stream = Some(stream);
        result.unwrap();
    }
}

// Copy paste of the options from async-io-typed.
fn bincode_options(size_limit: u64) -> impl Options {
    // Two of these are defaults, so you might say this is over specified. I say it's future proof, as
    // bincode default changes won't introduce accidental breaking changes.
    bincode::DefaultOptions::new()
        .with_limit(size_limit)
        .with_little_endian()
        .with_varint_encoding()
        .reject_trailing_bytes()
}

#[tokio::test]
async fn u16_marker_len_message() {
    let bincode_config = bincode_options(1024);

    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u16>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u16>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    let message = (0..248).map(|_| 1).chain(Some(300)).collect::<Vec<_>>();
    assert_eq!(bincode_config.serialize(&message).unwrap().len(), 252);
    let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
    assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
    fut.await.unwrap().1.unwrap();
}

#[tokio::test]
async fn u32_marker_len_message() {
    let bincode_config = bincode_options(1024);
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u16>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u16>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    let message = (0..249).map(|_| 1).chain(Some(300)).collect::<Vec<_>>();
    assert_eq!(bincode_config.serialize(&message).unwrap().len(), 253);
    let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
    assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
    fut.await.unwrap().1.unwrap();
}

#[tokio::test]
async fn u64_marker_len_message() {
    let bincode_config = bincode_options(1024);
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u8>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u8>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    let message = (0..251).collect::<Vec<_>>();
    assert_eq!(bincode_config.serialize(&message).unwrap().len(), 254);
    let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
    assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
    fut.await.unwrap().1.unwrap();
}

#[tokio::test]
async fn zst_marker_len_message() {
    let bincode_config = bincode_options(1024);
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u8>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u8>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    let message = (0..252).collect::<Vec<_>>();
    assert_eq!(bincode_config.serialize(&message).unwrap().len(), 255);
    let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
    assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
    fut.await.unwrap().1.unwrap();
}

// It takes a ridiculous amount of time to run the u64 tests
#[ignore]
#[tokio::test]
async fn u64_len_message() {
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u64>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u64>>::connect_with_limit(
        socket_name,
        u64::MAX,
    ));
    let mut server_stream = Some(listener.accept_with_limit(u64::MAX).await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    let message = (0..(u32::MAX as u64 + 1) / 8).collect::<Vec<_>>();
    let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
    assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
    fut.await.unwrap().1.unwrap();
}

#[ignore]
#[tokio::test]
async fn u64_len_messages() {
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u64>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u64>>::connect_with_limit(
        socket_name,
        u64::MAX,
    ));
    let mut server_stream = Some(listener.accept_with_limit(u64::MAX).await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    for _ in 0..10 {
        let message = (0..(u32::MAX as u64 + 1) / 8).collect::<Vec<_>>();
        let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
        assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
        let (stream, result) = fut.await.unwrap();
        server_stream = Some(stream);
        result.unwrap();
    }
}

#[tokio::test]
async fn random_len_test() {
    use rand::Rng;

    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u32>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u32>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    for _ in 0..800 {
        let message =
            (0..(rand::thread_rng().gen_range(0..(u8::MAX as u32 + 1) / 4))).collect::<Vec<_>>();
        let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
        assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
        let (stream, result) = fut.await.unwrap();
        server_stream = Some(stream);
        result.unwrap();
    }
}
