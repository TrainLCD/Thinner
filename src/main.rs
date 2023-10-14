use std::{
    env::{self, VarError},
    net::{AddrParseError, SocketAddr},
};

use axum::{extract::Query, routing::get, Router};
use hyper_tls::HttpsConnector;
use serde::Deserialize;
use station_api::Station;
use tonic_web::GrpcWebClientLayer;

use crate::station_api::station_api_client::StationApiClient;

pub mod station_api {
    tonic::include_proto!("app.trainlcd.grpc");
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Params {
    latitude: Option<f64>,
    longitude: Option<f64>,
    en: Option<bool>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    dotenv::from_filename(".env.local").ok();

    let addr = fetch_addr().unwrap();
    let app = Router::new().route("/nearby", get(nearby));
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn fetch_nearby(
    latitude: f64,
    longitude: f64,
) -> Result<Station, Box<dyn std::error::Error>> {
    let sapi_url = std::env::var("SAPI_URL").expect("SAPI_URL must be set.");

    let https = HttpsConnector::new();
    let client = hyper::Client::builder().build(https);

    let svc = tower::ServiceBuilder::new()
        .layer(GrpcWebClientLayer::new())
        .service(client);

    let mut client = StationApiClient::with_origin(svc, sapi_url.try_into()?);

    let request = tonic::Request::new(station_api::GetStationByCoordinatesRequest {
        latitude,
        longitude,
        limit: Some(1),
    });

    let response = client.get_stations_by_coordinates(request).await?;

    Ok(response.into_inner().stations[0].clone())
}

fn fetch_port() -> u16 {
    match env::var("PORT") {
        Ok(s) => s.parse().expect("Failed to parse $PORT"),
        Err(env::VarError::NotPresent) => {
            println!("$PORT is not set. Falling back to 3000.");
            3000
        }
        Err(VarError::NotUnicode(_)) => panic!("$PORT should be written in Unicode."),
    }
}

fn fetch_addr() -> Result<SocketAddr, AddrParseError> {
    let port = fetch_port();
    match env::var("HOST") {
        Ok(s) => format!("{}:{}", s, port).parse(),
        Err(env::VarError::NotPresent) => {
            let fallback_host = format!("[::1]:{}", port);
            println!("$HOST is not set. Falling back to {}.", fallback_host);
            fallback_host.parse()
        }
        Err(VarError::NotUnicode(_)) => panic!("$HOST should be written in Unicode."),
    }
}

async fn nearby(Query(params): Query<Params>) -> String {
    let Some(lat) = params.latitude else {
        return "ERROR! The parameter `latitude` isn't present.".to_string();
    };
    let Some(lon) = params.longitude else {
        return "ERROR! The parameter `longitude` isn't present.".to_string();
    };

    let station = fetch_nearby(lat, lon).await.unwrap();

    match params.en {
        Some(true) => station.name_roman.unwrap_or("".to_string()),
        Some(false) => station.name,
        None => station.name,
    }
}

mod h2c {
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };

    use hyper::{client::HttpConnector, Client};
    use tonic::body::BoxBody;
    use tower::Service;

    pub struct H2cChannel {
        pub client: Client<HttpConnector>,
    }

    impl Service<http::Request<BoxBody>> for H2cChannel {
        type Response = http::Response<hyper::Body>;
        type Error = hyper::Error;
        type Future =
            Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: http::Request<BoxBody>) -> Self::Future {
            let client = self.client.clone();

            Box::pin(async move {
                let origin = request.uri();

                let h2c_req = hyper::Request::builder()
                    .uri(origin)
                    .header(http::header::UPGRADE, "h2c")
                    .body(hyper::Body::empty())
                    .unwrap();

                let res = client.request(h2c_req).await.unwrap();

                if res.status() != http::StatusCode::SWITCHING_PROTOCOLS {
                    panic!("Our server didn't upgrade: {}", res.status());
                }

                let upgraded_io = hyper::upgrade::on(res).await.unwrap();

                // In an ideal world you would somehow cache this connection
                let (mut h2_client, conn) = hyper::client::conn::Builder::new()
                    .http2_only(true)
                    .handshake(upgraded_io)
                    .await
                    .unwrap();
                tokio::spawn(conn);

                h2_client.send_request(request).await
            })
        }
    }
}
