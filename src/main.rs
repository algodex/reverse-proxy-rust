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
use std::collections::HashSet;
use std::net::IpAddr;
use std::pin::Pin;
use std::thread::current;
use std::time::Duration;
use std::{convert::Infallible, net::SocketAddr};
use tokio::sync::RwLock;
use tokio::task;
use tokio::time::sleep;
use tokio::time::Instant;
type Uri = String;
use crate::UriCacheUpdateMessage::*;

#[derive(Clone, Debug)]
struct UriEntry {
    response_body: Option<String>,
    response_success: Option<bool>,
    is_fetching: bool,
    resp_headers: Option<HeaderMap>,
    fetch_complete_time: Option<Instant>,
    last_req_time: Instant,
    request_params: RequestParams,
    clear_timer_creation_time: Option<Instant>
}

#[derive(Clone, Debug)]
struct RequestParams {
    uri: String,
    queryStr: String,
    headerMap:HeaderMap,
    method:Method,
    body:String,
    request_etag: Option<HeaderValue>
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
    DeleteOrRefreshCache(String, Option<Instant>),
    SetClearTimerStart(String, Instant),
    FinishFetchingWithSuccess(String, UpdateCacheEntry),
    FinishFetchingWithError(String),
    UpdateLatestReqTimestamp(String)
}

#[derive(Clone, Debug)]
struct CachedResponseError {
    message:String
}


struct AppState {
    uri_cache: RwLock<HashMap<Uri, UriEntry>>,
    count: RwLock<u32>,
}

impl AppState {
    fn new() -> Self {
        let urlToResponse: HashMap<Uri, UriEntry> = HashMap::new();
        let mutex = RwLock::new(urlToResponse);
        let countMutex = RwLock::new(0);
        AppState {
            uri_cache: mutex,
            count: countMutex,
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

#[async_recursion]
async fn update_cache(msg: &UriCacheUpdateMessage) {
    // dbg!(msg);
    let mut uri_cache = APP_STATE.uri_cache.write().await;

    match msg {
        StartFetchingFromClient(uri, req_params) => {
            println!("StartFetchingFromClient: {uri}");
            let entry = UriEntry {
                response_body: None,
                is_fetching: true,
                response_success: None,
                resp_headers: None,
                fetch_complete_time: None,
                last_req_time: Instant::now(),
                request_params: (*req_params).clone(),
                clear_timer_creation_time: None
            };
            uri_cache.insert(uri.to_string(), entry);
        },
        SetClearTimerStart(uri, instant) => {
            if (uri_cache.get(uri).is_some()) {
                let current_item = uri_cache.get_mut(uri).unwrap();
                current_item.clear_timer_creation_time = Some(*instant);
            }
        },
        StartFetchingFromRefresh(uri) => {
            println!("StartFetchingFromRefresh: {uri}");
            let current_item = uri_cache.get(uri)
            .expect("Expected a cached item, but did not find one. Accidentally deleted? {uri}");

            let entry = UriEntry {
                response_body: current_item.response_body.clone(),
                is_fetching: true,
                response_success: None,
                resp_headers: current_item.resp_headers.clone(),
                fetch_complete_time: None,
                last_req_time: current_item.last_req_time,
                request_params: current_item.request_params.clone(),
                clear_timer_creation_time: None
            };
            uri_cache.insert(uri.to_string(), entry);
        }
        DeleteOrRefreshCache(uri, instant) => {
            println!("DeleteOrRefreshCache: {uri}");
            let cache_item = uri_cache.get(uri);

            let timer_mismatch_detected = 
            instant.is_some() &&  // We are here due to a timer
              (cache_item.is_none() || // cache item does not exist - makes no sense to have a timer to delete here
                (  (cache_item.unwrap().clear_timer_creation_time.is_some() && // Timer id was cached
                    instant.unwrap() != cache_item.unwrap().clear_timer_creation_time.unwrap()) // Timers mismatch
                || cache_item.unwrap().clear_timer_creation_time.is_none() // Or there was no timer set in cache item
                )
              );

            let env = ENV.read().await;
            let cache_refresh_window = env.get("DEFAULT_REFRESH_WINDOW_SECS").unwrap().parse::<u64>().unwrap();

            if cache_item.is_some() && cache_item.unwrap().is_fetching {
                // The cache is fetching, so no need to delete it - it will update soon
            } else if (cache_item.is_some() &&
                cache_item.unwrap().last_req_time.elapsed() <= Duration::from_secs(cache_refresh_window)
                && !timer_mismatch_detected) {
                    let request_params = cache_item.unwrap().request_params.clone();
                    let uri = uri.clone();
                    task::spawn(async move {
                        let req_count = incr_count().await;
                        println!("Refreshing Cache: {uri}");
                        background_refresh_cache(request_params.clone(), req_count, true).await;
                    });
            } else if (!timer_mismatch_detected) {
                println!("Deleting Cache (actually): {uri}");
                uri_cache.remove(uri);
            }
        },
        FinishFetchingWithSuccess(uri, update_cache_entry) => {
            println!("FinishFetchingWithSuccess: {uri}");
            let current_item = uri_cache.get(uri)
                .expect("Expected a cached item, but did not find one. Accidentally deleted? {uri}");

            let entry = UriEntry {
                response_body: update_cache_entry.response_body.clone(),
                response_success: Some(true),
                is_fetching: false,
                resp_headers: update_cache_entry.resp_headers.clone(),
                fetch_complete_time: Some(Instant::now()),
                last_req_time: current_item.last_req_time,
                request_params: current_item.request_params.clone(),
                clear_timer_creation_time: None
            };
            uri_cache.insert(uri.to_string(), entry);
        },
        FinishFetchingWithError(uri) => {
            println!("FinishFetchingWithError: {uri}");
            let current_item = uri_cache.get(uri)
                .expect("Expected a cached item, but did not find one. Accidentally deleted? {uri}");

            let entry = UriEntry {
                response_body: None,
                response_success: Some(false),
                is_fetching: false,
                resp_headers: None,
                fetch_complete_time: Some(Instant::now()),
                last_req_time: current_item.last_req_time,
                request_params: current_item.request_params.clone(),
                clear_timer_creation_time: None
            };
            uri_cache.insert(uri.to_string(), entry);
        },
        UpdateLatestReqTimestamp(uri) => {
            println!("UpdateLatestReqTimestamp: {uri}");
            let current_item = uri_cache.get_mut(uri);
            if current_item.is_some() {
                current_item.unwrap().last_req_time = Instant::now();
            }
        }
    }
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

async fn getCacheEntry(url: &Uri) -> Option<UriEntry> {
    let response_cache = &APP_STATE.uri_cache.read().await;

    if response_cache.contains_key(url) {
        let cached_response = response_cache.get(url).unwrap();
        return Some(cached_response.clone());
    }
    None
}

async fn getCachedResponseLoop(url: &Uri) -> Result<UriEntry, CachedResponseError> {
    loop {
        let response = getCacheEntry(url).await;
        match response {
            Some(val) => {
                if (val.response_body.is_some() && val.resp_headers.is_some()) {
                    return Ok(val);
                } else if (val.is_fetching) {
                    // do nothing. loop will continue
                } else if val.response_success.is_some() && val.response_success.unwrap() == false {
                    return Err(CachedResponseError{message: format!("Error during fetch: {url}")});
                } else {
                    return Err(CachedResponseError{message: format!("Error during fetch: {url} - Unknown condition")});
                }
                // else, continue the loop waiting
            }
            None => return Err(CachedResponseError{message: format!("No cache entry found for uri: {url}")}),
        }
    }
}
async fn getCachedResponseOrTimeout(url: &Uri) -> Result<UriEntry, CachedResponseError> {
    let cached_resp_fut = getCachedResponseLoop(url);
    let sleep_statement = task::spawn(delay());
    let res = tokio::select! {
        _ = sleep_statement => Err(CachedResponseError{message: format!("Time out while waiting for cached resp for uri: {url}")}),
        resp = cached_resp_fut => resp
    };
    res
}

fn build_response(body: &String, resp_headers: &HeaderMap, request_etag: &Option<HeaderValue>) -> Response<Body> {
    let status_code = match request_etag {
        None => StatusCode::OK,
        Some(req_etag) => {
            let status_code =
            if (resp_headers.contains_key("etag") &&
                  req_etag.to_str().unwrap() == resp_headers.get("etag").unwrap().to_str().unwrap()) {

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
    let app_state = &APP_STATE.uri_cache.read().await;
    let cache_item = app_state.get(uri);

    match cache_item {
        Some(item) => {
            item.is_fetching
        },
        None => false
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

    update_cache(&DeleteOrRefreshCache(clear_cache_url.to_string(), None)).await;
    println!("Deleted {clear_cache_url} from cache");

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(format!("Cleared Cache for {clear_cache_url}")))
        .unwrap())
  
}

async fn background_refresh_cache(request_params:RequestParams, count:u32, from_refresh:bool)
        -> Result<hyper::Response<Body>, Infallible> {

    let c_req_params = request_params.clone();
    let RequestParams {uri, queryStr, headerMap, method, body, request_etag} = request_params;

    if (from_refresh) {
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
    let fullURL = format!("{upstream_url}{uri_path}{queryStr}");
    println!("full URL: {fullURL}");

    let mut header_map_temp = headerMap.clone();
    header_map_temp.remove("if-none-match"); // We want upstream URLs to fetch full response
    let proxy_call = get_req(method, client, fullURL, body, header_map_temp).send();

    let res = tokio::select! {
        _ = sleep_statement => {
            println!("timed out while doing background fetch {uri_path}");
            update_cache(&FinishFetchingWithError(uri_path.clone())).await;
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
                        Err(_) => {
                            update_cache(&FinishFetchingWithError(uri_path.clone())).await;
                            println!("Error getting body from {}", &uri_path);
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
                        println!("{count}: Updating cache!");
                        let update_entry = UpdateCacheEntry {
                            resp_headers: Some(c_resp_headers),
                            response_body: Some(c_body)
                        };
                        
                        update_cache(&FinishFetchingWithSuccess(uri_c, update_entry)).await;
                        println!("{count}: Inserted into cache!");

                        task::spawn(async move {
                            let env = ENV.read().await;
                            let cache_expiry_time = env.get("DEFAULT_CACHE_EXPIRY_TIME_SECS").unwrap().parse::<u64>().unwrap();
                            let clear_timer_creation_time = Some(Instant::now());
                            let uri_c = uri.clone();
                            update_cache(&SetClearTimerStart(uri, clear_timer_creation_time.unwrap())).await;
                            sleep(Duration::from_secs(cache_expiry_time)).await;
                            {
                                println!("Clearing old cache for: {uri_c}");
                                update_cache(&DeleteOrRefreshCache(uri_c, clear_timer_creation_time)).await;
                            }
                        });
                    });

                    Ok(build_response(&proxy_text, &resp_headers, &request_etag))
                }
                Err(error) => {
                    println!("error in response from background fetch {uri_path} {error:?}");
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
    println!("in handle");

    let method = req.method().clone();
    let count = incr_count().await;
    println!("{count} requests");
    let body = read_json_body(&mut req).await;
    let pathAndQuery = req.uri().path_and_query();

    let _output: Vec<Bytes> = Vec::new();

    let query = pathAndQuery.unwrap().query();
    let path = pathAndQuery.unwrap().path();

    let headerMap: HeaderMap = req.headers().clone();
    println!("HEADERS: {:?}", headerMap);

    let request_etag = req.headers().get("if-none-match").cloned();

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
    let cached_resp = getCacheEntry(uri_path).await;
    if let Some(uri_entry) = cached_resp {
        let uri_c = uri_path.clone();
        task::spawn(async move {
            // This can be in another thread to not delay the response
            update_cache(&UpdateLatestReqTimestamp(uri_c)).await;
        });

        if uri_entry.response_body.is_some() {
            println!("{count} Returning from cache!");
            return Ok(build_response(&uri_entry.response_body.unwrap(),
                &uri_entry.resp_headers.unwrap(), &request_etag));
        }
    }

    println!("{count}: no cache found... {uri_path}");
    let is_fetching = is_fetching_uri(uri_path).await;

    if is_fetching {
        println!("{count}: could not get write lock. waiting for read lock");
        let cached_resp = getCachedResponseOrTimeout(uri_path).await;
        println!("{count}: got read lock");
        if cached_resp.is_ok() {
            let x = cached_resp.unwrap();
            return Ok(build_response(&x.response_body.unwrap(), &x.resp_headers.unwrap(), &request_etag));
        } else {
            return Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(cached_resp.unwrap_err().message))
                .unwrap());
        }
    }
    // Not currently in cache, so try to fetch and refresh cache

    return background_refresh_cache(RequestParams{
        uri: uri_path.clone(), queryStr, headerMap, method,
        body, request_etag}, count, false).await;
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
