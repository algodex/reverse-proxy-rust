// #![deny(warnings)]

// Author: Alexander Trefonas
// Copyright (c) 2022 Algodex VASP (BVI) Corp
// MIT License

#[macro_use]
extern crate lazy_static;

use async_recursion::async_recursion;
use hyper::body;
use hyper::body::Bytes;
use hyper::http::HeaderValue;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::Method; // 0.13.9
use hyper::{Body, HeaderMap, Request, Response, Server, StatusCode};
use reqwest::RequestBuilder;
use std::collections::HashMap;

use std::net::IpAddr;

use std::time::Duration;
use std::{convert::Infallible, net::SocketAddr};
use tokio::sync::RwLock;
use tokio::task;
use tokio::time::sleep;
use tokio::time::Instant;
type Uri = String;
use crate::UriCacheUpdateMessage::*;
extern crate queues;
use queues::*;

macro_rules! debug_dbg {
    ($($arg:tt)*) => (if ::std::cfg!(debug_assertions) { ::std::dbg!($($arg)*); })
}

macro_rules! debug_println {
    ($($arg:tt)*) => (if ::std::cfg!(debug_assertions) { ::std::println!($($arg)*); })
}

#[derive(Clone, Debug)]
struct UriEntry {
    response_body: Option<String>,
    response_success: Option<bool>,
    is_fetching: bool,
    resp_headers: Option<HeaderMap>,
    fetch_complete_time: Option<Instant>,
    last_req_time: Instant,
    request_params: RequestParams,
    clear_timer_creation_time: Option<Instant>,
}

#[derive(Clone, Debug)]
struct RequestParams {
    uri: String,
    query_str: String,
    header_map: HeaderMap,
    method: Method,
    body: String,
    request_etag: Option<HeaderValue>,
}

#[derive(Clone, Debug)]
struct UpdateCacheEntry {
    response_body: Option<String>,
    resp_headers: Option<HeaderMap>,
}

#[derive(Clone, Debug)]
enum UriCacheUpdateMessage {
    StartFetchingFromClient(String, RequestParams),
    StartFetchingFromRefresh(String),
    DeleteOrRefreshCache(String),
    SetClearTimerStart(String, Instant),
    FinishFetchingWithSuccess(String, UpdateCacheEntry),
    FinishFetchingWithError(String),
    UpdateLatestReqTimestamp(String),
}

#[derive(Clone, Debug)]
struct CachedResponseError {
    message: String,
}

struct UriCache {
    uri_map: HashMap<Uri, UriEntry>,
    active_uri_queue: Queue<Uri>
}

struct AppState {
    uri_cache: RwLock<UriCache>,
    count: RwLock<u32>,
}

impl AppState {
    fn new() -> Self {
        let url_to_response: HashMap<Uri, UriEntry> = HashMap::new();
        let active_uri_queue: Queue<Uri> = Queue::new();
        let uri_cache = UriCache {
            uri_map: url_to_response,
            active_uri_queue
        };

        let mutex = RwLock::new(uri_cache);
        let count_mutex = RwLock::new(0);
        AppState { uri_cache: mutex, count: count_mutex }
    }
}
lazy_static! {
    static ref APP_STATE: AppState = AppState::new();
    static ref ENV: RwLock<HashMap<String, String>> = {
        let m = HashMap::new();
        RwLock::new(m)
    };
}

#[async_recursion]
async fn update_cache(msg: &UriCacheUpdateMessage) {
    // debug_dbg!(msg);
    let mut uri_cache = APP_STATE.uri_cache.write().await;
    let uri_map = &mut uri_cache.uri_map;

    match msg {
        StartFetchingFromClient(uri, req_params) => {
            debug_println!("StartFetchingFromClient: {uri}");
            let entry = UriEntry {
                response_body: None,
                is_fetching: true,
                response_success: None,
                resp_headers: None,
                fetch_complete_time: None,
                last_req_time: Instant::now(),
                request_params: (*req_params).clone(),
                clear_timer_creation_time: None,
            };
            uri_map.insert(uri.to_string(), entry);
        }
        SetClearTimerStart(uri, instant) => {
            if uri_map.get(uri).is_some() {
                let current_item = uri_map.get_mut(uri).unwrap();
                current_item.clear_timer_creation_time = Some(*instant);
            }
        }
        StartFetchingFromRefresh(uri) => {
            debug_println!("StartFetchingFromRefresh: {uri}");
            let current_item = uri_map.get(uri).expect(
                "Expected a cached item, but did not find one. Accidentally deleted? {uri}",
            );

            let entry = UriEntry {
                response_body: current_item.response_body.clone(),
                is_fetching: true,
                response_success: None,
                resp_headers: current_item.resp_headers.clone(),
                fetch_complete_time: None,
                last_req_time: current_item.last_req_time,
                request_params: current_item.request_params.clone(),
                clear_timer_creation_time: None,
            };
            uri_map.insert(uri.to_string(), entry);
        }
        DeleteOrRefreshCache(uri) => {
            debug_println!("DeleteOrRefreshCache: {uri}");
            let cache_item = uri_map.get(uri);

            let env = ENV.read().await;
            let cache_refresh_window =
                env.get("DEFAULT_REFRESH_WINDOW_SECS").unwrap().parse::<u64>().unwrap();
            let req_timeout = env.get("REQ_TIMEOUT").unwrap().parse::<u64>().unwrap(); 
            if cache_item.is_some() && cache_item.unwrap().is_fetching 
                && cache_item.unwrap().last_req_time.elapsed() <= Duration::from_secs(req_timeout + 1) {
                debug_println!("Skipping delete/refresh: {uri}");
                // The cache is fetching, so no need to delete/refresh it - it will update soon
            } else if cache_item.is_some()
                && cache_item.unwrap().last_req_time.elapsed()
                    <= Duration::from_secs(cache_refresh_window)
            {
                let request_params = cache_item.unwrap().request_params.clone();
                let uri = uri.clone();
                task::spawn(async move {
                    let req_count = incr_count().await;
                    debug_println!("Refreshing Cache: {uri}");
                    background_refresh_cache(request_params.clone(), req_count, true).await;
                });
            } else {
                debug_println!("Deleting Cache (actually): {uri}");
                uri_map.remove(uri);
            }
        }
        FinishFetchingWithSuccess(uri, update_cache_entry) => {
            debug_println!("FinishFetchingWithSuccess: {uri}");
            let current_item = uri_map.get(uri).expect(
                "Expected a cached item, but did not find one. Accidentally deleted? {uri}",
            );

            let entry = UriEntry {
                response_body: update_cache_entry.response_body.clone(),
                response_success: Some(true),
                is_fetching: false,
                resp_headers: update_cache_entry.resp_headers.clone(),
                fetch_complete_time: Some(Instant::now()),
                last_req_time: current_item.last_req_time,
                request_params: current_item.request_params.clone(),
                clear_timer_creation_time: None,
            };
            uri_map.insert(uri.to_string(), entry);
        }
        FinishFetchingWithError(uri) => {
            debug_println!("FinishFetchingWithError: {uri}");
            let current_item = uri_map.get(uri).expect(
                "Expected a cached item, but did not find one. Accidentally deleted? {uri}",
            );

            let entry = UriEntry {
                response_body: None,
                response_success: Some(false),
                is_fetching: false,
                resp_headers: None,
                fetch_complete_time: Some(Instant::now()),
                last_req_time: current_item.last_req_time,
                request_params: current_item.request_params.clone(),
                clear_timer_creation_time: None,
            };
            uri_map.insert(uri.to_string(), entry);
        }
        UpdateLatestReqTimestamp(uri) => {
            debug_println!("UpdateLatestReqTimestamp: {uri}");
            let current_item = uri_map.get_mut(uri);
            if let Some(item) = current_item {
                item.last_req_time = Instant::now();
            }
        }
    }
}

// fn debug_request(req: Request<Body>) -> Result<Response<Body>, Infallible> {
//     let body_str = format!("{:?}", req);
//     Ok(Response::new(Body::from(body_str)))
// }

async fn delay() {
    let env = ENV.read().await;
    let timeout = env.get("REQ_TIMEOUT").unwrap().parse::<u64>().unwrap();
    sleep(Duration::from_secs(timeout)).await;
}

async fn get_cache_entry(url: &Uri) -> Option<UriEntry> {
    let response_cache = &APP_STATE.uri_cache.read().await;

    if response_cache.uri_map.contains_key(url) {
        let cached_response = response_cache.uri_map.get(url).unwrap();
        return Some(cached_response.clone());
    }
    None
}

async fn get_cached_response_loop(url: &Uri) -> Result<UriEntry, CachedResponseError> {
    loop {
        {
            let response = get_cache_entry(url).await;
            match response {
                Some(val) => {
                    if val.response_body.is_some() && val.resp_headers.is_some() {
                        return Ok(val);
                    } else if val.is_fetching {
                        // do nothing. loop will continue
                    } else if val.response_success.is_some() && !val.response_success.unwrap() {
                        return Err(CachedResponseError {
                            message: format!("Error during fetch: {url}"),
                        });
                    } else {
                        return Err(CachedResponseError {
                            message: format!("Error during fetch: {url} - Unknown condition"),
                        });
                    }
                    // else, continue the loop waiting
                }
                None => {
                    return Err(CachedResponseError {
                        message: format!("No cache entry found for uri: {url}"),
                    })
                }
            }
        }
        debug_println!("looping");
        //FIXME - change this to use a message channel instead of loop with polling
        sleep(Duration::from_millis(100)).await;
    }
}
async fn get_cached_response_or_timeout(url: &Uri) -> Result<UriEntry, CachedResponseError> {
    let cached_resp_fut = get_cached_response_loop(url);
    let sleep_statement = task::spawn(delay());
    let res = tokio::select! {
        _ = sleep_statement => Err(CachedResponseError{message: format!("Time out while waiting for cached resp for uri: {url}")}),
        resp = cached_resp_fut => resp
    };
    res
}

fn build_response(
    body: &String,
    resp_headers: &HeaderMap,
    request_etag: &Option<HeaderValue>,
) -> Response<Body> {
    let status_code = match request_etag {
        None => StatusCode::OK,
        Some(req_etag) => {
            let status_code = if resp_headers.contains_key("etag")
                && req_etag.to_str().unwrap() == resp_headers.get("etag").unwrap().to_str().unwrap()
            {
                StatusCode::from_u16(304).unwrap()
            } else {
                StatusCode::OK
            };
            status_code
        }
    };
    let mut builder = Response::builder().status(status_code);
    for header_key in resp_headers.keys() {
        builder = builder.header(header_key, resp_headers.get(header_key).unwrap());
    }

    let body = match status_code.as_u16() {
        304 => String::from(""),
        _ => String::from(body),
    };

    builder.body(Body::from(body)).unwrap()
}

async fn incr_count() -> u32 {
    let mut w = APP_STATE.count.write().await;
    *w += 1;
    *w
}

async fn is_fetching_uri(uri: &String) -> bool {
    let app_state = &APP_STATE.uri_cache.read().await;
    let cache_item = app_state.uri_map.get(uri);

    match cache_item {
        Some(item) => item.is_fetching,
        None => false,
    }
}


async fn delete_or_refresh_cache_loop() {
    let env = ENV.read().await;
    let cache_expiry_time = env.get("DEFAULT_CACHE_EXPIRY_TIME_SECS").unwrap().parse::<u64>().unwrap();

    loop {
        sleep(Duration::from_millis(500)).await;
        {
            let uri_cache = &APP_STATE.uri_cache.read().await;
            let queue = &uri_cache.active_uri_queue;
            if (queue.size() == 0) {
                continue;
            }
            let oldest_uri = queue.peek().unwrap();
            let cache_item = uri_cache.uri_map.get(&oldest_uri);
            if (cache_item.is_none()) {
                // The item is not in the uri_map, so remove from the queue
                let uri_cache = &mut APP_STATE.uri_cache.write().await;
                uri_cache.active_uri_queue.remove();
                continue;
            }

            let cache_item = cache_item.unwrap();

            let last_time = match cache_item.fetch_complete_time.is_some() {
                true => cache_item.fetch_complete_time.unwrap(),
                false => cache_item.last_req_time
            };
            if last_time.elapsed() > Duration::from_secs(cache_expiry_time) {
                debug_println!("Clearing/refreshing old cache for: {oldest_uri}");
                // Spawning because should be done in another thread to avoid locking?
                task::spawn(async move {
                    update_cache(&DeleteOrRefreshCache(oldest_uri)).await;
                });
            }           
        }
    }
}

pub async fn read_json_body(req: &mut Request<Body>) -> String {
    let body = req.body_mut();
    let bytes = body::to_bytes(body).await.unwrap();
    String::from_utf8(Vec::from(&*bytes)).unwrap()
}

// Box<dyn core::future::Future<Output = Result<reqwest::Response, reqwest::Error>>>
fn get_req(
    method: Method,
    client: reqwest::Client,
    full_url: String,
    body: String,
    header_map: HeaderMap,
) -> RequestBuilder {
    match method {
        Method::POST => client.post(full_url).body(body).headers(header_map),
        Method::GET => client.get(full_url).body(body).headers(header_map),
        _ => {
            //FIXME - add other types? Or return error?
            client.get(full_url).body(body).headers(header_map)
        }
    }
}

/// Clears the cache for a URL. CLEAR_CACHE_KEY must be set in the env.
/// Example:
/// curl http://localhost:8000/trades/history/asset/31566704 -H 'Clear-Cache: True' -H 'Clear-Cache-Key: MySecretKey'

async fn clear_cache(req: Request<Body>) -> Result<hyper::Response<Body>, Infallible> {
    let header_map = req.headers();
    let header_clear_cache_key = header_map.get("clear-cache-key");
    let env = ENV.read().await;
    let clear_cache_key = env.get("CLEAR_CACHE_KEY").unwrap();
    if header_clear_cache_key.is_none()
        || header_clear_cache_key.unwrap().to_str().unwrap() != clear_cache_key
    {
        return Ok(Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .body(Body::from("Unauthorized"))
            .unwrap());
    }

    let clear_cache_url = &req.uri().path().to_string();

    update_cache(&DeleteOrRefreshCache(clear_cache_url.to_string())).await;
    debug_println!("Deleted {clear_cache_url} from cache");

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(format!("Cleared Cache for {clear_cache_url}")))
        .unwrap())
}

async fn background_refresh_cache(
    request_params: RequestParams,
    count: u32,
    from_refresh: bool,
) -> Result<hyper::Response<Body>, Infallible> {
    let c_req_params = request_params.clone();
    let RequestParams {
        uri,
        query_str,
        header_map,
        method,
        body,
        request_etag,
    } = request_params;

    if from_refresh {
        update_cache(&StartFetchingFromRefresh(uri.clone())).await;
    } else {
        update_cache(&StartFetchingFromClient(uri.clone(), c_req_params)).await;
    }

    let uri_path = uri.clone(); //fixme - clean this up? not necessary

    let sleep_statement = task::spawn(delay());

    let client = reqwest::Client::new();

    let env = ENV.read().await;
    let upstream_url = env.get("UPSTREAM_URL").unwrap();

    //http://host.docker.internal:5984{uri_path}{queryStr}"
    let full_url = format!("{upstream_url}{uri_path}{query_str}");
    debug_println!("full URL: {full_url}");

    let mut header_map_temp = header_map.clone();
    header_map_temp.remove("if-none-match"); // We want upstream URLs to fetch full response
    let proxy_call = get_req(method, client, full_url, body, header_map_temp).send();

    let res = tokio::select! {
        _ = sleep_statement => {
            debug_println!("timed out while doing background fetch {uri_path}");
            update_cache(&FinishFetchingWithError(uri_path.clone())).await;
            {Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("timed out"))
                .unwrap())}
        },

        response = proxy_call => {
            match response {
                Ok(response) => {
                    debug_dbg!(&response);
                    let resp_headers = response.headers().clone();
                    let proxy_text = match response.text().await {
                        Ok(p) => {
                            // debug_println!("FULL RESPONSE:{}", p);
                            p
                        },
                        Err(_) => {
                            update_cache(&FinishFetchingWithError(uri_path.clone())).await;
                            debug_println!("Error getting body from {}", &uri_path);
                            return Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Body::from(format!("error response when getting body text from {uri_path}")))
                                .unwrap())
                        },
                    };
                    // Update Cache
                    let uri = uri_path.clone();
                    let c_body = proxy_text.clone();
                    let c_resp_headers = resp_headers.clone();
                    // Not sure if this should be in its own task
                    task::spawn(async move {
                        let uri_c = uri.clone();
                        debug_println!("{count}: Updating cache!");
                        let update_entry = UpdateCacheEntry {
                            resp_headers: Some(c_resp_headers),
                            response_body: Some(c_body)
                        };

                        update_cache(&FinishFetchingWithSuccess(uri_c, update_entry)).await;
                        debug_println!("{count}: Inserted into cache!");
                    });

                    Ok(build_response(&proxy_text, &resp_headers, &request_etag))
                }
                Err(error) => {
                    debug_println!("error in response from background fetch {uri_path} {error:?}");
                    update_cache(&FinishFetchingWithError(uri_path.clone())).await;
                    Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Body::from("error response"))
                                .unwrap())}
                }
            }
    };

    return res;
}

async fn handle(
    _client_ip: IpAddr,
    mut req: Request<Body>,
) -> Result<hyper::Response<Body>, Infallible> {
    debug_println!("in handle");

    let method = req.method().clone();
    let count = incr_count().await;
    debug_println!("{count} requests");
    let body = read_json_body(&mut req).await;
    let path_and_query = req.uri().path_and_query();

    let _output: Vec<Bytes> = Vec::new();

    let query = path_and_query.unwrap().query();
    let path = path_and_query.unwrap().path();

    let header_map: HeaderMap = req.headers().clone();
    debug_println!("HEADERS: {:?}", header_map);

    let request_etag = req.headers().get("if-none-match").cloned();

    if header_map.contains_key("clear-cache") {
        return clear_cache(req).await;
    }
    debug_println!("PATH: {path}");
    debug_println!("BODY: {body:?}");
    let query_str = match query {
        Some(q) => format!("?{q}"),
        None => "".to_string(),
    };
    if let Some(q) = query {
        debug_println!("QUERY: {q}");
    }
    let uri_path = &req.uri().path().to_string();
    let cached_resp = get_cache_entry(uri_path).await;
    if let Some(uri_entry) = cached_resp {
        let uri_c = uri_path.clone();
        task::spawn(async move {
            // This can be in another thread to not delay the response
            update_cache(&UpdateLatestReqTimestamp(uri_c)).await;
        });

        if uri_entry.response_body.is_some() {
            debug_println!("{count} Returning from cache!");
            return Ok(build_response(
                &uri_entry.response_body.unwrap(),
                &uri_entry.resp_headers.unwrap(),
                &request_etag,
            ));
        }
    }

    debug_println!("{count}: no cache found... {uri_path}");
    let is_fetching = is_fetching_uri(uri_path).await;

    if is_fetching {
        debug_println!("{count}: could not get write lock. waiting for read lock");
        let cached_resp = get_cached_response_or_timeout(uri_path).await;
        debug_println!("{count}: got read lock");
        if cached_resp.is_ok() {
            let x = cached_resp.unwrap();
            return Ok(build_response(
                &x.response_body.unwrap(),
                &x.resp_headers.unwrap(),
                &request_etag,
            ));
        } else {
            return Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(cached_resp.unwrap_err().message))
                .unwrap());
        }
    }
    // Not currently in cache, so try to fetch and refresh cache

    return background_refresh_cache(
        RequestParams { uri: uri_path.clone(), query_str, header_map, method, body, request_etag },
        count,
        false,
    )
    .await;
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

    task::spawn(async move {
        // Loop to refresh or delete cache
        delete_or_refresh_cache_loop().await;
    });

    if let Err(e) = server.await {
        println!("server error: {}", e);
    }
}
