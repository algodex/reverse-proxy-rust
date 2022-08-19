// #![deny(warnings)]
#[macro_use]
extern crate lazy_static;


use hyper::server::conn::AddrStream;
use hyper::{Body, Request, Response, Server, StatusCode};
use hyper::service::{service_fn, make_service_fn};
use std::{convert::Infallible, net::SocketAddr};
use std::net::IpAddr;
use tokio::sync::RwLock;
use std::time::Duration;
// use tokio_core::reactor::Core;
// use tokio_core::reactor::Handle;
// use futures::Future;
// use futures::future::Either;
// use tokio::sync::oneshot;
// use tokio::time::timeout;
use tokio::time::sleep;
use tokio::task;


struct AppState {
    messages: RwLock<Vec<String>>,
}
impl AppState {
    fn new() -> Self {
        let messages: Vec<String> = Vec::new();
        let mutex = RwLock::new(messages);
        AppState{messages: mutex}
    }
}
lazy_static! {
    // static ref my_mutex: Mutex<i32> = Mutex::new(0i32);
    static ref APP_STATE: AppState = AppState::new();
    // static ref handle = &Core::new().unwrap().handle();
}



fn debug_request(req: Request<Body>) -> Result<Response<Body>, Infallible>  {
    let body_str = format!("{:?}", req);
    Ok(Response::new(Body::from(body_str)))
}

async fn delay() {
    // Wait randomly for between 0 and 10 seconds
    sleep(Duration::from_secs(1)).await;
}

async fn handle(client_ip: IpAddr, req: Request<Body>) -> Result<Response<Body>, Infallible> {
    if req.uri().path().starts_with("/escrow") {
        // will forward requests to port 13901
        println!("handing: {}", req.uri().path());
        match hyper_reverse_proxy::call(client_ip, "http://127.0.0.1:5984", req).await {
            Ok(response) => {Ok(response)}
            Err(_error) => {Ok(Response::builder()
                                  .status(StatusCode::INTERNAL_SERVER_ERROR)
                                  .body(Body::empty())
                                  .unwrap())}
        }
    } else if req.uri().path().starts_with("/slow") {
        // let timeout = tokio_core::reactor::Timeout::new(Duration::from_millis(170), &handle).unwrap();
        let sleep_statement = task::spawn(delay());
        let proxy_call = hyper_reverse_proxy::call(client_ip, "http://127.0.0.1:8080", req);

        let res = tokio::select! {
            _ = sleep_statement => {
                {Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("timed out"))
                    .unwrap())}
            },

            response = proxy_call => {
                match response {
                    Ok(response) => {Ok(response)}
                    Err(_error) => {Ok(Response::builder()
                                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                                        .body(Body::empty())
                                        .unwrap())}
                    }
                }
        };

        res
        // // let mut r1 = APP_STATE.messages.write().await;
        // // let len = r1.len();
        // // let str = format!("hey {len}"); 
        // // will forward requests to port 13901
        // println!("handing: {}", req.uri().path());
        // match hyper_reverse_proxy::call(client_ip, "http://127.0.0.1:8080", req).await {
        //     Ok(response) => {Ok(response)}
        //     Err(_error) => {Ok(Response::builder()
        //                           .status(StatusCode::INTERNAL_SERVER_ERROR)
        //                           .body(Body::empty())
        //                           .unwrap())}
        // }
    } else {
        debug_request(req)
    }
}

#[tokio::main]
async fn main() {
    let bind_addr = "127.0.0.1:8000";
    let addr:SocketAddr = bind_addr.parse().expect("Could not parse ip:port.");

    let make_svc = make_service_fn(|conn: &AddrStream| {
        let remote_addr = conn.remote_addr().ip();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| handle(remote_addr, req)))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    println!("Running server on {:?}", addr);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}

/*
#![deny(warnings)]
#[macro_use]
extern crate lazy_static;

use std::convert::Infallible;
use std::net::SocketAddr;

use hyper::server::conn::Http;
use hyper::service::service_fn;
use hyper::{Body, Request, Response};
use tokio::net::TcpListener;
use tokio::sync::RwLock;


struct AppState {
    messages: RwLock<Vec<String>>,
}
impl AppState {
    fn new() -> Self {
        let messages: Vec<String> = Vec::new();
        let mutex = RwLock::new(messages);
        AppState{messages: mutex}
    }
}
lazy_static! {
    // static ref my_mutex: Mutex<i32> = Mutex::new(0i32);
    static ref APP_STATE: AppState = AppState::new();
}



async fn hello(_: Request<Body>) -> Result<Response<Body>, Infallible> {
    let mut r1 = APP_STATE.messages.write().await;
    let len = r1.len();
    let str = format!("hey {len}"); 
    r1.push("hello".to_string());
    Ok(Response::new(Body::from(str)))
}

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    pretty_env_logger::init();

    let addr: SocketAddr = ([127, 0, 0, 1], 3000).into();

    let listener = TcpListener::bind(addr).await?;
    // let lock = Arc::new(RwLock::new(1));

    println!("Listening on http://{}", addr);
    loop {
        let (stream, _) = listener.accept().await?;
        // let c_lock = lock.clone();
        tokio::task::spawn(async move {
            // let r1 = c_lock.read().await;
            // let val = *r1;
            if let Err(err) = Http::new()
                .serve_connection(stream, service_fn(hello))
                .await
            {
                println!("Error serving connection: {:?}", err);
            }
        });
    }
}

*/