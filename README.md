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

## Binary format

### Introduction

The main offering of this crate is a consistent and known representation of Rust types. As such, the format is 
considered to be part of our stable API, and changing the format requires a major version number bump. To aid you 
in debugging, that format is documented here.

### High-level overview

The byte stream is split up into messages. Every message begins with a `length` value. After `length` bytes have 
been read, a new message can begin immediately afterward. This `length` value is the entirety of the header of a 
message. Messages have no footer. The bytes read out of a message are then deserialized into a Rust type via
[`bincode`](https://github.com/bincode-org/bincode), using the following configuration

```rust
bincode::DefaultOptions::new()
    .with_limit(size_limit)
    .with_little_endian()
    .with_fixint_encoding()
    .reject_trailing_bytes()
```

### Length encoding

The length is encoded using a variably sized integer encoding scheme. To understand this scheme, first we need a few constant values.

```
u16_marker; decimal: 252, hex: FC
u32_marker; decimal: 253, hex: FD
u64_marker; decimal: 254, hex: FE
zst_marker; decimal: 255, hex: FF
stream_end; decimal: 0,   hex: 00
```

Any length less than `u16_marker` and greater than 0 is encoded as a single byte whose value is the length.
A length of zero is encoded with the `zst_marker`. The stream is ended with the `stream_end` value. When this is
read the peer is expected to close the connection.

`interprocess-typed` always uses little-endian. The user data being sent may contain values that are not 
little-endian, but `interprocess-typed` itself always uses little-endian.

If the first byte is `u16_marker`, then the length is 16 bits wide, and encoded in the following 2 bytes. Once
those 2 bytes are read, the message begins. `u32_marker` and `u64_marker` are used in a similar way, each of 
those being 4 bytes, and 8 bytes respectively.

### Examples


Length 12
```
0C
```

Length 0
```
FF
```

Length 252 (First byte is u16_marker)
```
FC, FC, 00
```

Length 253 (First byte is u16_marker)
```
FC, FD, 00
```

Length 65,536 (aka 2^16) (First byte is u32_marker)
```
FD, 00, 00, 01, 00
```

Length 4,294,967,296 (aka 2^32) (First byte is u64_marker)
```
FE, 00, 00, 00, 00, 01, 00, 00, 00
```