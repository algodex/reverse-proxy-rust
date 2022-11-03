# reverse-proxy-rust

Reverse proxy for serving web requests to the Algodex 2.0 backend. This uses asynchronous green threads via tokio-rs to serve responses from an in-memory HashMap cache. On a cache miss, only a single request will go to the upstream service to process the data, regardless of how many clients are requesting the same URL. The cache expiration time is configurable in the .env file. Additionally, it has support for clearing particular cache keys, so that the upstream service can clear particular cached requests when it knows the data needs to be refreshed.

This also features a Docker setup with nginx in front, which allows further configuration such as gzip encoding. Additionally, it supports 304 responses (etag's).

## Requirements

- Rust
- Docker

## Build instructions

### Install Rust

`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

### Copy and configure .env file

`cp .env.example .env`

### Build

`cargo build --release`

## Running

`./start.sh`
