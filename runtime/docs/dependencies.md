# Dependency policy

The runtime builds on any target with nothing installed behind it. No system
OpenSSL, no system zlib, no cmake, no pkg-config. That is a hard rule, checked by
`tests/purity.rs`, which reads the resolved tree and fails on the crates that
break it.

## What is in the tree now

| need | crate | why |
| --- | --- | --- |
| async, fs, net | smol, futures-lite | small, no tokio |
| real sockets, SO_REUSEPORT | socket2 | needs the socket options |
| hashing | sha1, sha2, md-5, hmac | RustCrypto, pure Rust |
| randomness | getrandom | talks to the OS directly |
| URL parsing | url | idna included, pure Rust |
| TS/TSX compile | oxc | pure Rust, already in the tree |

The engine is quickjs-oxide, which is also pure Rust.

## What to pick when fetch and the rest of crypto land

| need | crate | notes |
| --- | --- | --- |
| TLS | rustls with the rustls-rustcrypto provider | zero C at build time: no ring, no aws-lc. ring was the first choice but it compiles vendored C and asm, and a C toolchain in the build is the thing we are avoiding. rustls-rustcrypto is younger, so TLS gets its own tests rather than trust. |
| gzip, deflate | flate2 with default-features off, feature rust_backend | that routes to miniz_oxide, pure Rust. The default backend is already miniz_oxide but the C one is one feature flag away, so pin it. |
| brotli | brotli-decompressor | pure Rust |
| AES, GCM, CBC | aes, aes-gcm, cbc, ctr | RustCrypto |
| password hashing | pbkdf2, scrypt, argon2 | RustCrypto |
| signatures, key objects | rsa, p256, p384, ed25519-dalek | pure Rust, but the key parsing needs care |

Nothing in the tree may need a C compiler. That rules out ring as well, so TLS
is rustls plus rustls-rustcrypto: the primitives come from the same RustCrypto
crates the crypto module already uses.

## Never

`openssl`, `openssl-sys`, `native-tls`, `security-framework`, `schannel`,
`aws-lc-sys`, `libz-sys`, `zstd-sys`. Each of those either links a system
library or turns the build into a platform-specific exercise.
