use std::{
    convert::Infallible,
    fmt,
    io::{self, BufReader},
    net::SocketAddr,
    str::FromStr,
    sync::Arc,
};

use http_body_util::Full;
use hyper::{body::Bytes, server::conn::http1, service::service_fn, Method, Request, Response};
use hyper_util::rt::TokioIo;
use rustls::{Certificate, PrivateKey};
use rustls_pemfile::{certs, pkcs8_private_keys};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

macro_rules! respond_text {
    ($v:expr) => {
        Full::new(Bytes::from($v.trim().to_string()))
    };
}

macro_rules! redirect {
    ($destination:expr) => {
        Ok(Response::builder()
            .header("location", $destination)
            .status(301)
            .body(respond_text!(""))
            .unwrap())
    };
}

#[cfg(not(debug_assertions))]
const MAIN_DOMAIN: &str = "kingland.id";

enum FaviconType {
    AppleTouch,
    Favicon16,
    Favicon32,
}

struct FaviconTypeParseError;

impl FromStr for FaviconType {
    type Err = FaviconTypeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_lowercase();

        if s.contains("apple") {
            return Ok(Self::AppleTouch);
        }

        if s.contains("favicon16") {
            return Ok(Self::Favicon16);
        }

        if s.contains("favicon32") {
            return Ok(Self::Favicon32);
        }

        Err(FaviconTypeParseError)
    }
}

async fn service(
    req: Request<hyper::body::Incoming>,
    protocol: Protocol,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let host = req.headers().get("host").unwrap().to_str().unwrap();
    let uri = req.uri().to_string();

    #[cfg(not(debug_assertions))]
    {
        if !host.contains(MAIN_DOMAIN) {
            return redirect!(format!("{protocol}://{MAIN_DOMAIN}{uri}"));
        }
        if !host.contains("www.") {
            return redirect!(format!("{protocol}://www.{MAIN_DOMAIN}{uri}"));
        }
        if protocol == Protocol::HTTP {
            return redirect!(format!("https://{host}{uri}"));
        }
    }

    match (req.method(), req.uri().path()) {
        (&Method::GET, "/favicon") => {
            let result = (|| {
                let query = if let Some(query) = req.uri().query() {
                    query
                } else {
                    return None;
                };

                if !query.contains("t=") {
                    return None;
                }

                let mut favicon_type = query.split_once("t=").unwrap().1;

                if favicon_type.contains("&") {
                    favicon_type = favicon_type.split_once("&").unwrap().0;
                }

                if let Ok(favicon_type) = favicon_type.parse::<FaviconType>() {
                    match favicon_type {
                        FaviconType::AppleTouch => {
                            return Some(Bytes::from(
                                &include_bytes!("../public/favicons/apple-touch-icon.png")[..],
                            ));
                        }
                        FaviconType::Favicon16 => {
                            return Some(Bytes::from(
                                &include_bytes!("../public/favicons/favicon-16x16.png")[..],
                            ));
                        }
                        FaviconType::Favicon32 => {
                            return Some(Bytes::from(
                                &include_bytes!("../public/favicons/favicon-32x32.png")[..],
                            ));
                        }
                    }
                }

                None
            })();

            if let Some(result) = result {
                return Ok(Response::new(Full::new(result)));
            }
        }
        (&Method::GET, "/discord" | "/dc" | "/invite" | "/invites") => {
            return redirect!("https://discord.gg/PEsARGFup7");
        }
        _ => {}
    }

    Ok(Response::new(respond_text!(include_str!(
        "../public/index.html"
    ))))
}

#[derive(Debug, PartialEq)]
enum Protocol {
    HTTP,
    HTTPS,
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Protocol::HTTP => {
                write!(f, "http")
            }
            Protocol::HTTPS => {
                write!(f, "https")
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let http_addr = SocketAddr::from(([0, 0, 0, 0], 80));
    let https_addr = SocketAddr::from(([0, 0, 0, 0], 443));

    #[cfg(debug_assertions)]
    let http_addr = SocketAddr::from(([10, 184, 0, 2], 80));

    let http_listener = TcpListener::bind(http_addr)
        .await
        .expect("Failed binding the listener");

    #[cfg(not(debug_assertions))]
    let https_listener = TcpListener::bind(https_addr)
        .await
        .expect("Failed binding the listener");

    let certs = certs(&mut BufReader::new(
        include_str!("../certs/certificate.crt").as_bytes(),
    ))
    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cert"))
    .map(|mut certs| certs.drain(..).map(Certificate).collect())
    .unwrap();

    let mut keys: Vec<PrivateKey> = pkcs8_private_keys(&mut BufReader::new(
        include_str!("../certs/private.key").as_bytes(),
    ))
    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid key"))
    .map(|mut keys| keys.drain(..).map(PrivateKey).collect())
    .unwrap();

    let config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, keys.remove(0))
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));

    tokio::spawn(async move {
        loop {
            let (http_stream, _) = http_listener
                .accept()
                .await
                .expect("Failed accepting http connection");

            tokio::spawn(async move {
                let io = TokioIo::new(http_stream);

                if let Err(err) = http1::Builder::new()
                    .serve_connection(io, service_fn(|r| service(r, Protocol::HTTP)))
                    .await
                {
                    eprintln!("Something is wrong: {:?}", err)
                }
            });
        }
    });

    #[cfg(not(debug_assertions))]
    tokio::spawn(async move {
        loop {
            let (https_stream, _) = https_listener
                .accept()
                .await
                .expect("Failed accepting https connection");

            let acceptor = acceptor.clone();

            tokio::spawn(async move {
                let stream = acceptor.accept(https_stream).await.expect("Invalid TLS");
                let io = TokioIo::new(stream);

                if let Err(err) = http1::Builder::new()
                    .serve_connection(io, service_fn(|r| service(r, Protocol::HTTPS)))
                    .await
                {
                    eprintln!("Something is wrong: {:?}", err)
                }
            });
        }
    });

    loop {}
}
