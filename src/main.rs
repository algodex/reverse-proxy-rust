// #![deny(warnings)]

// Author: Alexander Trefonas
// Copyright (c) 2022 Algodex VASP (BVI) Corp
// MIT License

#[macro_use]
extern crate lazy_static;

use hyper::body;
use hyper::body::Bytes;
use hyper::http::HeaderValue;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::Method; // 0.13.9
use hyper::{Body, HeaderMap, Request, Response, Server, StatusCode};
use reqwest::RequestBuilder;
use std::collections::HashMap;
use std::collections::HashSet;
use std::net::IpAddr;
use std::time::Duration;
use std::{convert::Infallible, net::SocketAddr};
use tokio::sync::RwLock;
use tokio::task;
use tokio::time::sleep;
use tokio::time::Instant;
type Uri = String;

#[derive(Clone, Debug)]
struct CachedResponse {
    body: String,
    resp_headers: HeaderMap,
    time: Instant,
}

struct AppState {
    response_cache: RwLock<HashMap<Uri, CachedResponse>>,
    is_fetching_set: RwLock<HashSet<Uri>>,
    count: RwLock<u32>,
}
impl AppState {
    fn new() -> Self {
        let urlToResponse: HashMap<Uri, CachedResponse> = HashMap::new();
        let is_fetching_set: HashSet<Uri> = HashSet::new();
        let mutex = RwLock::new(urlToResponse);
        let countMutex = RwLock::new(0);
        let is_fetching_set = RwLock::new(is_fetching_set);
        AppState {
            response_cache: mutex,
            count: countMutex,
            is_fetching_set,
        }
    }
}
lazy_static! {
    static ref APP_STATE: AppState = AppState::new();
    static ref ENV: RwLock<HashMap<String, String>> = {
        let m = HashMap::new();
        RwLock::new(m)
    };
}

fn debug_request(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let body_str = format!("{:?}", req);
    Ok(Response::new(Body::from(body_str)))
}

async fn delay() {
    let env = ENV.read().await;
    let timeout = env.get("REQ_TIMEOUT").unwrap().parse::<u64>().unwrap();
    sleep(Duration::from_secs(timeout)).await;
}

async fn getCachedResponseLoop(url: &Uri) -> Option<CachedResponse> {
    loop {
        let response = getCachedResponse(url).await;
        match response {
            Some(val) => {
                return Some(val);
            }
            None => sleep(Duration::from_millis(250)).await,
        }
    }
}
async fn getCachedResponseOrTimeout(url: &Uri) -> Option<CachedResponse> {
    let cached_resp_fut = getCachedResponseLoop(url);
    let sleep_statement = task::spawn(delay());
    let res = tokio::select! {
        _ = sleep_statement => None,
        resp = cached_resp_fut => resp
    };
    res
}

async fn getCachedResponse(url: &Uri) -> Option<CachedResponse> {
    let response_cache = &APP_STATE.response_cache.read().await;

    if response_cache.contains_key(url) {
        let cached_response = response_cache.get(url).unwrap();
        return Some(cached_response.clone());
    }
    return None;
}

fn build_response(body: &String, headers: &HeaderMap, request_etag: &Option<&HeaderValue>) -> Response<Body> {
    let status_code = match request_etag {
        None => StatusCode::OK,
        Some(req_etag) => {
            let mut status_code:StatusCode;
            if (headers.contains_key("etag") &&
                req_etag.to_str().unwrap() == headers.get("etag").unwrap().to_str().unwrap()) {
                status_code = StatusCode::from_u16(304).unwrap();
            } else {
                status_code = StatusCode::OK;
            }
            status_code
        }
    };
    let mut builder = Response::builder().status(status_code);
    for header_key in headers.keys() {
        builder = builder.header(header_key, headers.get(header_key).unwrap());
    }

    let body = match status_code.as_u16() {
        304 => String::from(""),
        _ => String::from(body)
    };

    builder.body(Body::from(body)).unwrap()
}


async fn incr_count() -> u32 {
    let mut w = APP_STATE.count.write().await;
    *w += 1;
    *w
}

async fn is_fetching_uri(uri: &String) -> bool {
    let is_fetching_set = &APP_STATE.is_fetching_set.read().await;
    return is_fetching_set.contains(uri);
}

async fn set_is_fetching_uri(uri: &String, is_fetching: bool) {
    let mut is_fetching_set = APP_STATE.is_fetching_set.write().await;
    if is_fetching {
        is_fetching_set.insert(uri.clone());
    } else {
        is_fetching_set.remove(uri);
    }
}

pub async fn read_json_body(req: &mut Request<Body>) -> String {
    let body = req.body_mut();
    let bytes = body::to_bytes(body).await.unwrap();
    let body_str = String::from_utf8(Vec::from(&*bytes)).unwrap();
    body_str
}

// Box<dyn core::future::Future<Output = Result<reqwest::Response, reqwest::Error>>>
fn get_req(
    method: Method,
    client: reqwest::Client,
    fullURL: String,
    body: String,
    headerMap: HeaderMap,
) -> RequestBuilder {
    let proxy_call = match method {
        Method::POST => client.post(fullURL).body(body).headers(headerMap),
        Method::GET => client.get(fullURL).body(body).headers(headerMap),
        _ => {
            //FIXME - add other types? Or return error?
            client.get(fullURL).body(body).headers(headerMap)
        }
    };
    proxy_call
}

/// Clears the cache for a URL. CLEAR_CACHE_KEY must be set in the env.
/// Example: 
/// curl http://localhost:8000/trades/history/asset/31566704 -H 'Clear-Cache: True' -H 'Clear-Cache-Key: MySecretKey'

async fn clear_cache(mut req: Request<Body>) -> Result<hyper::Response<Body>, Infallible> {
    let header_map = req.headers();
    let header_clear_cache_key = header_map.get("clear-cache-key");
    let env = ENV.read().await;
    let clear_cache_key = env.get("CLEAR_CACHE_KEY").unwrap();
    if (header_clear_cache_key.is_none() ||
        header_clear_cache_key.unwrap().to_str().unwrap() != clear_cache_key) {
        return Ok(Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .body(Body::from("Unauthorized"))
            .unwrap());
    }

    let clear_cache_url = &req.uri().path().to_string();

    let mut response_cache = APP_STATE.response_cache.write().await;
    response_cache.remove(clear_cache_url);
    println!("Deleted {clear_cache_url} from cache");

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(format!("Cleared Cache for {clear_cache_url}")))
        .unwrap())
  
}

async fn handle(
    _client_ip: IpAddr,
    mut req: Request<Body>,
) -> Result<hyper::Response<Body>, Infallible> {
    println!("in handle");

    let method = req.method().clone();
    let count = incr_count().await;
    println!("{count}: here");
    let body = read_json_body(&mut req).await;
    let pathAndQuery = req.uri().path_and_query();

    let _output: Vec<Bytes> = Vec::new();

    let query = pathAndQuery.unwrap().query();
    let path = pathAndQuery.unwrap().path();

    let headerMap: HeaderMap = req.headers().clone();
    println!("HEADERS: {:?}", headerMap);

    let request_etag = req.headers().get("if-none-match");

    if (headerMap.contains_key("clear-cache")) {
        return clear_cache(req).await;
    }
    println!("PATH: {path}");
    println!("BODY: {body:?}");
    let queryStr = match query {
        Some(q) => format!("?{q}"),
        None => "".to_string(),
    };
    if let Some(q) = query {
        println!("QUERY: {q}");
    }
    let uri_path = &req.uri().path().to_string();
    let cached_resp = getCachedResponse(uri_path).await;
    if cached_resp.is_some() {
        println!("{count} Returning from cache!");
        let x = cached_resp.unwrap();
        return Ok(build_response(&x.body, &x.resp_headers, &request_etag));
    }

    println!("{count}: no cache found... {uri_path}");
    let is_fetching = is_fetching_uri(&uri_path).await;

    if is_fetching {
        println!("{count}: could not get write lock. waiting for read lock");
        let cached_resp = getCachedResponseOrTimeout(uri_path).await;
        println!("{count}: got read lock");
        if cached_resp.is_some() {
            let x = cached_resp.unwrap();
            return Ok(build_response(&x.body, &x.resp_headers, &request_etag));
        } else {
            return Ok(build_response(
                &"Timed out while getting response".to_string(),
                &HeaderMap::new(),
                &request_etag
            ));
        }
    }
    // Not currently in cache, so try to fetch and refresh cache

    set_is_fetching_uri(uri_path, true).await;

    let sleep_statement = task::spawn(delay());

    let client = reqwest::Client::new();

    let env = ENV.read().await;
    let upstream_url = env.get("UPSTREAM_URL").unwrap();

    //http://host.docker.internal:5984{uri_path}{queryStr}"
    let fullURL = format!("{upstream_url}{uri_path}{queryStr}");
    println!("full URL: {fullURL}");

    let mut header_map_temp = headerMap.clone();
    header_map_temp.remove("if-none-match"); // We want upstream URLs to fetch full response
    let proxy_call = get_req(method, client, fullURL, body, header_map_temp).send();

    let res = tokio::select! {
        _ = sleep_statement => {
            println!("timed out while doing background fetch {uri_path}");
            set_is_fetching_uri(&uri_path, false).await;
            {Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("timed out"))
                .unwrap())}
        },

        response = proxy_call => {
            match response {
                Ok(response) => {
                    dbg!(&response);
                    let resp_headers = response.headers().clone();
                    let proxy_text = match response.text().await {
                        Ok(p) => {
                            // println!("FULL RESPONSE:{}", p);
                            p
                        },
                        Err(_) => {return Ok(Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Body::from(format!("error response when getting body text from {uri_path}")))
                            .unwrap())},
                    };
                    // Update Cache
                    let uri = uri_path.clone();
                    let c_body = proxy_text.clone();
                    let c_resp_headers = resp_headers.clone();
                    // Not sure if this should be in its own task
                    task::spawn(async move {
                        let uri_c = uri.clone();
                        println!("{count}: Updating cache!");
                        {
                            let mut response_cache = APP_STATE.response_cache.write().await;
                            let cached_resp = CachedResponse {
                                body: c_body,
                                resp_headers: c_resp_headers,
                                time: Instant::now()
                            };
                            set_is_fetching_uri(&uri, false).await;
                            response_cache.insert(uri, cached_resp);
                            println!("{count}: Inserted into cache!");
                        }

                        task::spawn(async move {
                            let env = ENV.read().await;
                            let cache_expiry_time = env.get("DEFAULT_CACHE_EXPIRY_TIME_SECS").unwrap().parse::<u64>().unwrap();
                            sleep(Duration::from_secs(cache_expiry_time)).await;
                            {
                                let mut response_cache = APP_STATE.response_cache.write().await;
                                response_cache.remove(&uri_c);
                                println!("Cache is old, removed {uri_c} from cache");
                            }
                        });
                    });
                    Ok(build_response(&proxy_text, &resp_headers, &request_etag))
                }
                Err(error) => {
                    println!("error in response from background fetch {uri_path} {error:?}");
                    set_is_fetching_uri(&uri_path, false).await;
                    Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Body::from("error response"))
                                .unwrap())}
                }
            }
    };

    res
}

#[tokio::main]
async fn main() {
    dotenv::from_filename(".env").expect(".env file can't be found!");
    {
        let mut env = ENV.write().await;
        dotenv::vars().for_each(|val| {
            env.insert(val.0, val.1);
        });
    }

    let port = 8000;
    let bind_addr = format!("0.0.0.0:{port}");
    let addr: SocketAddr = bind_addr.parse().expect("Could not parse ip:port.");

    let make_svc = make_service_fn(|conn: &AddrStream| {
        let remote_addr = conn.remote_addr().ip();
        async move { Ok::<_, Infallible>(service_fn(move |req| handle(remote_addr, req))) }
    });

    let server = Server::bind(&addr).serve(make_svc);

    println!("Running server on {:?}", addr);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
