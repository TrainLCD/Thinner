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

    let lines = station
        .lines
        .iter()
        .map(|l| match params.en {
            Some(true) => l.name_roman.clone().unwrap_or("".to_string()),
            _ => l.name_short.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");

    match params.en {
        Some(true) => format!(
            "{}\n{}",
            station.name_roman.unwrap_or("".to_string()),
            lines
        ),
        _ => format!("{}\n{}", station.name, lines),
    }
}
