# interprocess-typed

![ci status](https://github.com/Xaeroxe/interprocess-typed/actions/workflows/rust.yml/badge.svg)

A wrapper over the [`interprocess`](https://github.com/kotauskas/interprocess) crate which allows
[`serde`](https://github.com/serde-rs/serde) compatible types to be sent over a [`tokio`](https://github.com/tokio-rs/tokio)
local socket with async.

## Who needs this?

Anyone who wishes to communicate between two processes running on the same system. This crate supports all the same systems as `interprocess`,
which at time of writing includes

- Windows
- MacOS
- Linux
- Other Unix platforms such as BSDs, illumos, and Solaris

Unlike similar offerings in the Rust ecosystem, this crate supports having multiple clients connected to one 
server, and so is appropriate for use in daemons.

## Why shouldn't I just use `interprocess` directly?

It depends on what you want to send! `interprocess` transmits raw streams of bytes, with no message delimiters or 
format. `interprocess-typed` allows you to send Rust types instead. Specifically, types that are serde-compatible. 
If your data is more easily described as a stream of bytes then you should use the `interprocess` crate directly. 
However, if you'd rather work with complex types, this  crate suits your needs better.

## Alternative async runtimes

`interprocess-typed` only supports `tokio` and has no plans to support alternate `async` runtimes.

## Contributing

Contributions are welcome! Please ensure your changes to the code pass unit tests. If you're fixing a bug please
add a unit test so that someone doesn't un-fix the bug later. This crate is kept intentionally minimalist, so most
feature requests would fit better in a crate that uses this one as a dependency.

## Binary Format

This crate uses [`async-io-typed`](https://github.com/Xaeroxe/async-io-typed) to transform types into bytes.
Information on the binary format can be found there.