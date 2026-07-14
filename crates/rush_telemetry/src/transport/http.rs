//! Zero-dependency HTTP/1.1 client for telemetry dispatch.
//!
//! Uses only `std::net::TcpStream` — no hyper, no reqwest, no TLS.
//! Rush Linux's deployment model assumes a trusted LAN or TLS
//! termination at the network edge (e.g., reverse proxy).
//!
//! The client sends a single HTTP/1.1 POST with the signed, compressed
//! telemetry envelope as the body. Single retry on connection failure.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Default connection timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Default read timeout.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Maximum retries.
const MAX_RETRIES: u32 = 1;

/// HTTP response from the telemetry endpoint.
#[derive(Debug)]
pub struct HttpResponse {
    pub status_code: u16,
    pub body: Vec<u8>,
}

/// Zero-dependency telemetry HTTP client.
pub struct TelemetryClient {
    /// Endpoint URL (host:port only, no scheme).
    endpoint: String,
    /// URL path for the telemetry API.
    path: String,
}

impl TelemetryClient {
    /// Create a new client targeting the given endpoint.
    ///
    /// `endpoint` should be `host:port` (e.g., "collect.rush.local:8080").
    /// `path` is the URL path (e.g., "/api/telemetry").
    pub fn new(endpoint: &str, path: &str) -> Self {
        TelemetryClient {
            endpoint: endpoint.to_string(),
            path: path.to_string(),
        }
    }

    /// Send a signed telemetry envelope via HTTP POST.
    ///
    /// Returns the HTTP response. Retries once on connection failure.
    pub fn send(&self, envelope: &[u8]) -> io::Result<HttpResponse> {
        let mut last_err = None;

        for attempt in 0..=MAX_RETRIES {
            match self.try_send(envelope) {
                Ok(response) => return Ok(response),
                Err(e) => {
                    log::warn!(
                        "Telemetry send attempt {}/{} failed: {e}",
                        attempt + 1,
                        MAX_RETRIES + 1
                    );
                    last_err = Some(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "telemetry send failed")
        }))
    }

    /// Single HTTP POST attempt.
    fn try_send(&self, body: &[u8]) -> io::Result<HttpResponse> {
        // Connect with timeout
        let mut stream = TcpStream::connect(&self.endpoint)?;
        stream.set_write_timeout(Some(READ_TIMEOUT))?;
        stream.set_read_timeout(Some(READ_TIMEOUT))?;

        // Build HTTP/1.1 request (hand-rolled, no dependencies)
        let request = format!(
            "POST {path} HTTP/1.1\r\n\
             Host: {host}\r\n\
             Content-Type: application/x-rush-telemetry\r\n\
             Content-Encoding: zstd\r\n\
             Content-Length: {length}\r\n\
             Connection: close\r\n\
             \r\n",
            path = self.path,
            host = self.endpoint,
            length = body.len(),
        );

        // Send request
        stream.write_all(request.as_bytes())?;
        stream.write_all(body)?;
        stream.flush()?;

        // Read response
        let mut response_buf = Vec::with_capacity(4096);
        let mut chunk = [0u8; 4096];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => response_buf.extend_from_slice(&chunk[..n]),
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }

        // Parse HTTP response
        parse_http_response(&response_buf)
    }
}

/// Parse a raw HTTP response into status code and body.
fn parse_http_response(raw: &[u8]) -> io::Result<HttpResponse> {
    // Find the end of headers
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "malformed HTTP response")
        })?;

    let headers = std::str::from_utf8(&raw[..header_end])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // Parse status line
    let status_line = headers.lines().next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "empty HTTP response")
    })?;

    // "HTTP/1.1 200 OK" → extract status code
    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "cannot parse HTTP status")
        })?
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let body = raw[header_end + 4..].to_vec();

    Ok(HttpResponse { status_code, body })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_http_response() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let response = parse_http_response(raw).unwrap();
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body, b"hello");
    }

    #[test]
    fn test_parse_http_error_response() {
        let raw = b"HTTP/1.1 404 Not Found\r\n\r\n";
        let response = parse_http_response(raw).unwrap();
        assert_eq!(response.status_code, 404);
        assert!(response.body.is_empty());
    }

    #[test]
    fn test_parse_malformed_response() {
        let raw = b"garbage data";
        assert!(parse_http_response(raw).is_err());
    }
}
