// #![deny(warnings)]
#[macro_use]
extern crate lazy_static;


// use futures::StreamExt;
use hyper::server::conn::AddrStream;
use hyper::{Body, Request, Response, Server, StatusCode, HeaderMap, header};
use hyper::service::{service_fn, make_service_fn};
use reqwest::RequestBuilder;
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
use std::collections::HashMap;
use tokio::time::Instant;
use std::collections::HashSet;
use hyper::{ Method}; // 0.13.9
use futures::{future, FutureExt, TryStreamExt};
use hyper::body;
use std::error::Error;
use hyper::body::Bytes;
use futures::StreamExt;
// let body = reqwest::get("https://www.rust-lang.org")
//     .await?
//     .text()
//     .await?;
type Uri = String;


const MAX_CACHE_TIME_SECS: u64 = 3;
const REQ_TIMEOUT: u64 = 5;

#[derive(Clone, Debug)]
struct CachedResponse {
    body: String,
    time: Instant
}

struct AppState {
    response_cache: RwLock<HashMap<Uri,CachedResponse>>,
    is_fetching_set: RwLock<HashSet<Uri>>,
    count: RwLock<u32>
}
impl AppState {
    fn new() -> Self {
        let urlToResponse: HashMap<Uri,CachedResponse> = HashMap::new();
        let is_fetching_set: HashSet<Uri> = HashSet::new();
        let mutex = RwLock::new(urlToResponse);
        let countMutex = RwLock::new(0);
        let is_fetching_set = RwLock::new(is_fetching_set);
        AppState{response_cache: mutex, count: countMutex, is_fetching_set}
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
    sleep(Duration::from_secs(REQ_TIMEOUT)).await;
}


async fn getCachedResponseLoop(url: &Uri) -> Option<CachedResponse> {
    loop {
        let response = getCachedResponse(url).await;
        match response {
            Some(val) => {
                return Some(val);
            },
            None => sleep(Duration::from_millis(250)).await
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

    if (response_cache.contains_key(url)) {
        let cached_response = response_cache.get(url).unwrap();
        return Some(cached_response.clone());
    }
    return None;
}

fn build_response(body: &String) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(String::from(body)))
        .unwrap()
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
    if (is_fetching) {
        is_fetching_set.insert(uri.clone());
    } else {
        is_fetching_set.remove(uri);
    }
}

pub async fn read_json_body(req: &mut Request<Body>) -> String {
    let mut body = req.body_mut();
    let bytes = body::to_bytes(body).await.unwrap();
    let body_str = String::from_utf8(Vec::from(&*bytes)).unwrap();
    body_str
}

// Box<dyn core::future::Future<Output = Result<reqwest::Response, reqwest::Error>>> 
fn get_req(method: Method, client: reqwest::Client, fullURL: String, body: String, headerMap: HeaderMap) -> RequestBuilder
    {
    let proxy_call = match method {
        POST => {
            client.post(fullURL)
            .body(body).headers(headerMap)
        },
        GET => {
            client.get(fullURL)
            .body(body).headers(headerMap)
        },
        _ => { //FIXME - add other types? Or return error?           
            client.get(fullURL)
            .body(body).headers(headerMap)
        }
    };
    proxy_call
}

async fn handle(_client_ip: IpAddr, mut req: Request<Body>) -> Result<hyper::Response<Body>, Infallible> {
    println!("in handle");

    let method = req.method().clone();
    let count = incr_count().await;
    println!("{count}: here");
    // let timeout = tokio_core::reactor::Timeout::new(Duration::from_millis(170), &handle).unwrap();
    let body = read_json_body(&mut req).await;
    let pathAndQuery = req.uri().path_and_query();

    let mut output:Vec<Bytes> = Vec::new();
    
    // let body_aggr = hyper::body::aggregate(body).await.unwrap();
    // let s = String::from_utf8_lossy(body_aggr);
    // match req.read_to_end(&mut body) {
    //     Ok(_) => println!("{:?}", String::from_utf8_lossy(&*body)),
    //     Err(why) => panic!("String conversion failure: {:?}", why),
    // }
    // let body = get_body_as_vec(body).await.unwrap();
    let query = pathAndQuery.unwrap().query();
    let path = pathAndQuery.unwrap().path();

    let headerMap: HeaderMap = req.headers().clone();
    println!("HEADERS: {:?}", headerMap);
    println!("PATH: {path}");
    println!("BODY: {body:?}");
    let queryStr = match query {
        Some(q) => format!("?{q}"),
        None => "".to_string()
    };
    if let Some(q) = query {
        println!("QUERY: {q}");
    }
    let uri_path = &req.uri().path().to_string();
    let cached_resp = getCachedResponse(uri_path).await;
    if cached_resp.is_some() {
        println!("{count} Returning from cache!");
        let x = cached_resp.unwrap();
        return Ok(build_response(&x.body));
    }
    
    println!("{count}: no cache found... {uri_path}");
    let is_fetching = is_fetching_uri(&uri_path).await;

    if (is_fetching) {
        println!("{count}: could not get write lock. waiting for read lock");
        let cached_resp = getCachedResponseOrTimeout(uri_path).await;
        println!("{count}: got read lock");
        if cached_resp.is_some() {
            let x = cached_resp.unwrap();
            return Ok(build_response(&x.body));
        } else {
            return Ok(build_response(&"Timed out while getting response".to_string()));
        }
    }
    // Not currently fetching, so try to fetch and refresh cache

    //FIXME - set is fetching
    set_is_fetching_uri(uri_path, true).await;

    let sleep_statement = task::spawn(delay());

    //let proxy_call = reqwest::get(uri); FIXME
    //let proxy_call = reqwest::get(format!("http://localhost:8080{uri_path}")); //FIXME - dont hardcode to localhost
    let client = reqwest::Client::new();

    let fullURL = format!("http://localhost:5984{uri_path}{queryStr}");
    println!("full URL: {fullURL}");

    // let proxy_call = reqwest::Request::new(*method, reqwest::Url::parse(&fullURL)
    //     .unwrap());

    // let created_req = reqwest::Request {
    //     method,
    //     url: reqwest::Url::parse(&fullURL).unwrap(),
    //     headers: headerMap,
    //     body: Some(body.into()),
    //     timeout: None,
    //     version: reqwest::Version::default(),
    // };
    // let proxy_call = client.execute(created_req);


    let proxy_call = get_req(method, client, fullURL, body, headerMap).send();

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
                    let proxy_text = match response.text().await {
                        Ok(p) => {
                            println!("FULL RESPONSE:{}", p);
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
                    // Not sure if this should be in its own task
                    task::spawn(async move {
                        println!("{count}: Updating cache!");
                        let mut response_cache = APP_STATE.response_cache.write().await;
                        let cached_resp = CachedResponse {
                            body: c_body,
                            time: Instant::now()
                        };
                        set_is_fetching_uri(&uri, false).await;
                        let uri_c = uri.clone();
                        response_cache.insert(uri, cached_resp);
                        println!("{count}: Inserted into cache!");

                        task::spawn(async move {
                            delay().await;
                            let mut response_cache = APP_STATE.response_cache.write().await;
                            response_cache.remove(&uri_c);
                            println!("Cache is old, removed {uri_c} from cache");
                        });
                    });
                    Ok(build_response(&proxy_text))
                }
                    Err(_error) => {
                        println!("error in response from background fetch {uri_path}");
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
    let port = 8000;
    let bind_addr = format!("127.0.0.1:{port}");
    let addr:SocketAddr = bind_addr.parse().expect("Could not parse ip:port.");

    let make_svc = make_service_fn(|conn: &AddrStream| {
        let remote_addr = conn.remote_addr().ip();
        async move {
            Ok::<_, Infallible>(service_fn(move |mut req| {
                handle(remote_addr, req)
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    println!("Running serverr on {:?}", addr);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}

