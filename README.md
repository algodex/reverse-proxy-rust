# reverse-proxy-rust

## Description

Generalized reverse proxy for serving web requests, and is currently in-use for the Algodex backend. This uses asynchronous green threads via tokio-rs to serve responses from an in-memory HashMap cache. On a cache miss, only a single request will go to the upstream service to process the data, regardless of how many clients are requesting the same URL. 

The cache expiration time is configurable in the .env file. Additionally, it has support for refreshing and clearing particular cache keys, and also allows the upstream service to clear or refresh the proxy's cache even before the timeout expires, via a private endpoint configurable with a password set in the header. The environment variable DEFAULT_REFRESH_WINDOW_SECS is used to determine whether the cache is refreshed or cleared after DEFAULT_CACHE_EXPIRY_TIME_SECS, or from an on-demand cache clear/refresh request from the upstream service.

It also supports 304 responses (etag's) when the client sends the 'If-None-Match' header, which is useful for the clients to poll for new data.

The basic purpose of this reverse proxy is so that clients can hit the server with large amounts of similar requests, and it will serve all of them with low latency and up-to-date response data.

This also features a Docker setup with nginx in front, which allows further configuration such as gzip encoding. 

## Basic Architecture

#### Cache miss

```client -> nginx (port 8080) -> reverse-proxy-rust (port 8000) -> upstream service```

#### Cache hit

```client -> nginx (port 8080) -> reverse-proxy-rust (port 8000)```

#### On-demand cache refreshing/clearing from the upstream service:

```
1. reverse-proxy-rust (port 8000) <- upstream service (request from upstream service to
                                                       trigger initial refresh or clearing)
2. reverse-proxy-rust -> upstream service (request from reverse-proxy to upstream service
                                           for latest data, triggered by step 1 above)
``` 

Note: `CLEAR_CACHE_KEY` and `CLEAR_CACHE` must be set in the request headers. Although it is called "clear cache", it actually means refresh or clear cache, depending on when the last client request was and the DEFAULT_REFRESH_WINDOW_SECS setting. 

## Requirements

- Rust
- Docker

## Build instructions

### Install Rust

`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

### Copy and configure .env file

`cp .env.example .env`

### Build and Run as Standalone

`cargo run`

## Build and Run with Nginx In Docker

`./start.sh`
