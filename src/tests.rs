use futures::{SinkExt, StreamExt};
use tokio::task::JoinHandle;

use super::*;

fn start_send_helper<T: Serialize + Unpin + Send + 'static>(
    mut s: LocalSocketStreamTyped<T>,
    value: T,
) -> JoinHandle<LocalSocketStreamTyped<T>> {
    tokio::spawn(async move {
        s.send(value).await.unwrap();
        s
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
    fut.await.unwrap();
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
    fut.await.unwrap();
    assert!(client_stream.next().await.is_none());
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
        server_stream = Some(fut.await.unwrap());
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
    fut.await.unwrap();
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
        server_stream = Some(fut.await.unwrap());
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
    fut.await.unwrap();
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
        server_stream = Some(fut.await.unwrap());
    }
}

#[ignore]
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
    fut.await.unwrap();
}

#[ignore]
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
        server_stream = Some(fut.await.unwrap());
    }
}

// It takes a ridiculous amount of RAM to run the u64 tests
#[ignore]
#[tokio::test]
async fn u64_len_message() {
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u64>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u64>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    let message = (0..(u32::MAX as u64 + 1) / 8).collect::<Vec<_>>();
    let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
    assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
    fut.await.unwrap();
}

#[ignore]
#[tokio::test]
async fn u64_len_messages() {
    let socket_name = generate_socket_name().unwrap();
    let listener = LocalSocketListenerTyped::<Vec<u64>>::bind(socket_name.as_os_str()).unwrap();
    let client_stream = tokio::spawn(LocalSocketStreamTyped::<Vec<u64>>::connect(socket_name));
    let mut server_stream = Some(listener.accept().await.unwrap());
    let mut client_stream = client_stream.await.unwrap().unwrap();
    for _ in 0..10 {
        let message = (0..(u32::MAX as u64 + 1) / 8).collect::<Vec<_>>();
        let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
        assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
        server_stream = Some(fut.await.unwrap());
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
            (0..(rand::thread_rng().gen_range(0..(u16::MAX as u32 + 1) / 4))).collect::<Vec<_>>();
        let fut = start_send_helper(server_stream.take().unwrap(), message.clone());
        assert_eq!(client_stream.next().await.unwrap().unwrap(), message);
        server_stream = Some(fut.await.unwrap());
    }
}
