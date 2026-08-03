use std::{collections::BTreeMap, sync::Arc, time::Duration};

use rhai::{Array, Dynamic, Engine, EvalAltResult, Map, Module};
use ureq::{
    Agent, Error as UreqError, Proxy,
    http::{HeaderName, HeaderValue, Method, Request, Uri, Version},
};

use crate::{
    script_process::ScriptDuration,
    script_stdlib::ScriptBytes,
    script_stream::{ScriptStream, from_bounded_reader},
    script_task::{ScriptTask, start_http_task},
};

pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(2);
pub const MAX_HTTP_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_HTTP_BODY_BYTES: usize = 64 * 1024;
pub const MAX_HTTP_BODY_BYTES: usize = 256 * 1024;
pub const MAX_HTTP_REQUEST_BODY_BYTES: usize = 256 * 1024;
pub const MAX_HTTP_HEADERS: usize = 64;
pub const MAX_HTTP_HEADER_BYTES: usize = 32 * 1024;
pub const MAX_HTTP_URL_BYTES: usize = 8 * 1024;
pub const DEFAULT_HTTP_REDIRECTS: u32 = 5;
pub const MAX_HTTP_REDIRECTS: u32 = 10;

#[derive(Clone)]
pub(crate) struct ScriptHttpResponse {
    status: i64,
    version: &'static str,
    headers: Arc<BTreeMap<String, Vec<Vec<u8>>>>,
    body: ScriptStream,
}

impl std::fmt::Debug for ScriptHttpResponse {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("version", &self.version)
            .field("header_names", &self.headers.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
struct HttpRequestSpec {
    method: Method,
    uri: Uri,
    headers: Vec<(HeaderName, HeaderValue)>,
    body: Vec<u8>,
    timeout: Duration,
    max_body_bytes: usize,
    max_redirects: u32,
    proxy: ProxySetting,
}

#[derive(Clone)]
enum ProxySetting {
    Environment,
    Disabled,
    Explicit(Proxy),
}

pub(crate) fn register(engine: &mut Engine, rhai_module: &mut Module) {
    engine.register_type_with_name::<ScriptHttpResponse>("HttpResponse");
    engine.register_get("status", |response: &mut ScriptHttpResponse| {
        response.status
    });
    engine.register_get("version", |response: &mut ScriptHttpResponse| {
        response.version.to_owned()
    });
    engine.register_get("headers", response_headers);
    engine.register_get("body", |response: &mut ScriptHttpResponse| {
        response.body.clone()
    });
    engine.register_fn("header", response_header);

    let mut http = Module::new();
    http.set_native_fn("request", http_request_default);
    http.set_native_fn("request", http_request);
    http.set_native_fn("start", http_start_default);
    http.set_native_fn("start", http_start);
    rhai_module.set_sub_module("http", http);
}

fn http_request_default(method: &str, url: &str) -> Result<ScriptHttpResponse, Box<EvalAltResult>> {
    http_request(method, url, Map::new())
}

fn http_request(
    method: &str,
    url: &str,
    options: Map,
) -> Result<ScriptHttpResponse, Box<EvalAltResult>> {
    let spec = parse_request(method, url, options)?;
    perform_request(spec).map_err(Into::into)
}

fn http_start_default(method: &str, url: &str) -> Result<ScriptTask, Box<EvalAltResult>> {
    http_start(method, url, Map::new())
}

fn http_start(method: &str, url: &str, options: Map) -> Result<ScriptTask, Box<EvalAltResult>> {
    let spec = parse_request(method, url, options)?;
    start_http_task(move || perform_request(spec))
}

fn parse_request(
    method: &str,
    url: &str,
    mut options: Map,
) -> Result<HttpRequestSpec, Box<EvalAltResult>> {
    if method.len() > 16 {
        return Err("http_method_invalid: method is too long".into());
    }
    let method = Method::from_bytes(method.as_bytes())
        .map_err(|_| "http_method_invalid: method is not a valid HTTP token")?;
    if !matches!(
        method,
        Method::GET
            | Method::POST
            | Method::PUT
            | Method::PATCH
            | Method::DELETE
            | Method::HEAD
            | Method::OPTIONS
    ) {
        return Err("http_method_unsupported: method is outside the v0.1.9 client set".into());
    }

    if url.is_empty() || url.len() > MAX_HTTP_URL_BYTES || url.contains(['\r', '\n', '\0']) {
        return Err("http_url_invalid: URL is empty, oversized, or contains controls".into());
    }
    let uri = url
        .parse::<Uri>()
        .map_err(|_| "http_url_invalid: URL could not be parsed")?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) || uri.authority().is_none() {
        return Err("http_url_invalid: only absolute HTTP(S) URLs are supported".into());
    }

    let headers = parse_request_headers(options.remove("headers"))?;
    let body = parse_request_body(options.remove("body"))?;
    let timeout = parse_duration_option(
        options.remove("timeout"),
        DEFAULT_HTTP_TIMEOUT,
        "http_timeout_option",
    )?;
    let max_body_bytes = parse_usize_option(
        options.remove("max_body_bytes"),
        DEFAULT_HTTP_BODY_BYTES,
        1,
        MAX_HTTP_BODY_BYTES,
        "http_max_body_bytes",
    )?;
    let max_redirects = u32::try_from(parse_usize_option(
        options.remove("max_redirects"),
        DEFAULT_HTTP_REDIRECTS as usize,
        0,
        MAX_HTTP_REDIRECTS as usize,
        "http_max_redirects",
    )?)
    .map_err(|_| "http_max_redirects: value overflow")?;
    let proxy = parse_proxy(options.remove("proxy"))?;
    if !options.is_empty() {
        return Err("http_option_unknown: unsupported request option".into());
    }

    Ok(HttpRequestSpec {
        method,
        uri,
        headers,
        body,
        timeout,
        max_body_bytes,
        max_redirects,
        proxy,
    })
}

fn parse_request_headers(
    value: Option<Dynamic>,
) -> Result<Vec<(HeaderName, HeaderValue)>, Box<EvalAltResult>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let headers = value
        .try_cast::<Map>()
        .ok_or("http_headers_type: headers must be a map")?;
    if headers.len() > MAX_HTTP_HEADERS {
        return Err(format!("http_headers_limit: maximum is {MAX_HTTP_HEADERS}").into());
    }
    let mut result = Vec::new();
    let mut bytes = 0_usize;
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| "http_header_name: invalid header name")?;
        let values = if value.is::<String>() {
            vec![
                value
                    .into_string()
                    .map_err(|_| "http_header_value: expected string")?,
            ]
        } else {
            value
                .try_cast::<Array>()
                .ok_or("http_header_value: expected string or string array")?
                .into_iter()
                .map(|value| {
                    value
                        .into_string()
                        .map_err(|_| "http_header_value: array items must be strings")
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        if values.is_empty() {
            return Err("http_header_value: value array must not be empty".into());
        }
        for value in values {
            bytes = bytes.saturating_add(name.as_str().len() + value.len());
            if bytes > MAX_HTTP_HEADER_BYTES || result.len() >= MAX_HTTP_HEADERS {
                return Err(format!(
                    "http_headers_limit: maximum is {MAX_HTTP_HEADER_BYTES} bytes"
                )
                .into());
            }
            let value = HeaderValue::from_str(&value)
                .map_err(|_| "http_header_value: invalid header value")?;
            result.push((name.clone(), value));
        }
    }
    Ok(result)
}

fn parse_request_body(value: Option<Dynamic>) -> Result<Vec<u8>, Box<EvalAltResult>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let body = if value.is::<String>() {
        value
            .into_string()
            .map_err(|_| "http_body_type: expected text or Bytes")?
            .into_bytes()
    } else {
        value
            .try_cast::<ScriptBytes>()
            .ok_or("http_body_type: expected text or Bytes")?
            .0
    };
    if body.len() > MAX_HTTP_REQUEST_BODY_BYTES {
        return Err(format!(
            "http_request_body_limit: maximum is {MAX_HTTP_REQUEST_BODY_BYTES} bytes"
        )
        .into());
    }
    Ok(body)
}

fn parse_duration_option(
    value: Option<Dynamic>,
    default: Duration,
    code: &str,
) -> Result<Duration, Box<EvalAltResult>> {
    let Some(value) = value else {
        return Ok(default);
    };
    let duration = value
        .try_cast::<ScriptDuration>()
        .map(|value| value.0)
        .ok_or_else(|| -> Box<EvalAltResult> {
            format!("{code}: expected std::time::Duration").into()
        })?;
    if duration.is_zero() || duration > MAX_HTTP_TIMEOUT {
        return Err(format!(
            "{code}: duration must be greater than zero and at most {} ms",
            MAX_HTTP_TIMEOUT.as_millis()
        )
        .into());
    }
    Ok(duration)
}

fn parse_usize_option(
    value: Option<Dynamic>,
    default: usize,
    minimum: usize,
    maximum: usize,
    code: &str,
) -> Result<usize, Box<EvalAltResult>> {
    let Some(value) = value else {
        return Ok(default);
    };
    let value = value
        .as_int()
        .map_err(|_| format!("{code}: expected integer"))?;
    let value = usize::try_from(value).map_err(|_| format!("{code}: value is out of range"))?;
    if value < minimum || value > maximum {
        return Err(format!("{code}: value must be between {minimum} and {maximum}").into());
    }
    Ok(value)
}

fn parse_proxy(value: Option<Dynamic>) -> Result<ProxySetting, Box<EvalAltResult>> {
    let Some(value) = value else {
        return Ok(ProxySetting::Environment);
    };
    if value.is::<bool>() {
        return if value.as_bool().unwrap_or(true) {
            Err("http_proxy_option: true is ambiguous; use a URL or false".into())
        } else {
            Ok(ProxySetting::Disabled)
        };
    }
    let value = value
        .into_string()
        .map_err(|_| "http_proxy_option: expected URL text or false")?;
    let proxy = Proxy::new(&value).map_err(|_| "http_proxy_invalid: proxy URL is invalid")?;
    Ok(ProxySetting::Explicit(proxy))
}

fn perform_request(spec: HttpRequestSpec) -> Result<ScriptHttpResponse, String> {
    let tls = crate::platform::script_http::tls_config().map_err(str::to_owned)?;
    let mut config = Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(spec.timeout))
        .max_redirects(spec.max_redirects)
        .max_response_header_size(64 * 1024)
        .tls_config(tls);
    config = match spec.proxy {
        ProxySetting::Environment => config,
        ProxySetting::Disabled => config.proxy(None),
        ProxySetting::Explicit(proxy) => config.proxy(Some(proxy)),
    };
    let agent = config.build().new_agent();

    let mut request = Request::builder().method(spec.method).uri(spec.uri);
    for (name, value) in spec.headers {
        request = request.header(name, value);
    }
    let request = request
        .body(spec.body)
        .map_err(|_| "http_request_invalid".to_owned())?;
    let response = agent.run(request).map_err(map_ureq_error)?;
    let (parts, body) = response.into_parts();
    let headers = response_headers_native(&parts.headers);
    let status = i64::from(parts.status.as_u16());
    let version = version_name(parts.version);
    let body = from_bounded_reader(body.into_reader(), "bytes", 0, spec.max_body_bytes)
        .map_err(|error| error.to_string())?;
    Ok(ScriptHttpResponse {
        status,
        version,
        headers: Arc::new(headers),
        body,
    })
}

fn response_headers_native(
    headers: &ureq::http::HeaderMap<HeaderValue>,
) -> BTreeMap<String, Vec<Vec<u8>>> {
    let mut result = BTreeMap::<String, Vec<Vec<u8>>>::new();
    for (name, value) in headers {
        result
            .entry(name.as_str().to_owned())
            .or_default()
            .push(value.as_bytes().to_vec());
    }
    result
}

fn response_headers(response: &mut ScriptHttpResponse) -> Map {
    response
        .headers
        .iter()
        .map(|(name, values)| {
            let values = values
                .iter()
                .cloned()
                .map(|value| Dynamic::from(ScriptBytes(value)))
                .collect::<Array>();
            (name.clone().into(), Dynamic::from(values))
        })
        .collect()
}

fn response_header(
    response: &mut ScriptHttpResponse,
    name: &str,
) -> Result<Array, Box<EvalAltResult>> {
    if name.is_empty() || name.len() > 256 || !name.is_ascii() {
        return Err("http_header_name: invalid lookup name".into());
    }
    Ok(response
        .headers
        .get(&name.to_ascii_lowercase())
        .into_iter()
        .flatten()
        .cloned()
        .map(|value| Dynamic::from(ScriptBytes(value)))
        .collect())
}

fn version_name(version: Version) -> &'static str {
    match version {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_11 => "HTTP/1.1",
        Version::HTTP_2 => "HTTP/2",
        Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/unknown",
    }
}

fn map_ureq_error(error: UreqError) -> String {
    let code = match error {
        UreqError::StatusCode(_) => "http_status",
        UreqError::Http(_) | UreqError::BadUri(_) => "http_request_invalid",
        UreqError::Protocol(_) | UreqError::BodyStalled => "http_protocol",
        UreqError::Io(ref error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) =>
        {
            "http_timeout"
        }
        UreqError::Io(ref error) if error.kind() == std::io::ErrorKind::InvalidData => "http_tls",
        UreqError::Io(_) | UreqError::ConnectionFailed | UreqError::Other(_) => "http_transport",
        UreqError::Timeout(_) => "http_timeout",
        UreqError::HostNotFound => "http_host_not_found",
        UreqError::RedirectFailed | UreqError::TooManyRedirects => "http_redirect",
        UreqError::InvalidProxyUrl | UreqError::ConnectProxyFailed(_) => "http_proxy",
        UreqError::BodyExceedsLimit(_) => "http_request_body_limit",
        UreqError::Tls(_) | UreqError::Pem(_) | UreqError::TlsRequired => "http_tls",
        UreqError::RequireHttpsOnly(_) => "http_scheme",
        UreqError::LargeResponseHeader(_, _) => "http_header_limit",
        // Cargo enables exactly one provider feature on each target. Every
        // feature-independent ureq error is classified above, leaving only
        // the selected provider's Rustls or NativeTls/DER variants here.
        _ => "http_tls",
    };
    code.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn loopback_server(response: &'static [u8]) -> (String, thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + content_length {
                    break;
                }
            }
            stream.write_all(response).unwrap();
            request
        });
        (format!("http://{address}/fixture"), handle)
    }

    fn engine() -> Engine {
        let mut engine = Engine::new();
        crate::script_stdlib::register_local(&mut engine);
        engine
    }

    #[test]
    fn loopback_request_preserves_status_headers_body_and_post_bytes() {
        let (url, server) = loopback_server(
            b"HTTP/1.1 201 Created\r\nX-Test: one\r\nX-Test: two\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
        );
        let source = format!(
            r#"
                let response = rhai::http::request("POST", "{url}", #{{
                    headers: #{{"content-type": "text/plain", "x-secret": "sentinel"}},
                    body: "payload",
                    proxy: false,
                    timeout: std::time::Duration::from_secs(1),
                    max_body_bytes: 16
                }});
                let values = response.header("x-test");
                #{{
                    status: response.status,
                    version: response.version,
                    first_header: values[0].to_text(),
                    body: response.body.collect(16).to_text()
                }}
            "#
        );
        let result = engine().eval::<Map>(&source).unwrap();
        assert_eq!(result["status"].as_int().unwrap(), 201);
        assert_eq!(result["version"].clone().into_string().unwrap(), "HTTP/1.1");
        assert_eq!(result["first_header"].clone().into_string().unwrap(), "one");
        assert_eq!(result["body"].clone().into_string().unwrap(), "hello");
        let request = String::from_utf8(server.join().unwrap()).unwrap();
        assert!(request.starts_with("POST /fixture HTTP/1.1\r\n"));
        assert!(request.contains("\r\nx-secret: sentinel\r\n"));
        assert!(request.ends_with("\r\n\r\npayload"));
    }

    #[test]
    fn body_limit_and_configuration_errors_are_typed() {
        let (url, server) = loopback_server(
            b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nabcdefgh",
        );
        let source = format!(
            r#"
                let response = rhai::http::request("GET", "{url}", #{{
                    proxy: false, max_body_bytes: 4
                }});
                let body = response.body;
                let text = body.collect(16).to_text();
                #{{text: text, truncated: body.truncated, complete: body.complete}}
            "#
        );
        let result = engine().eval::<Map>(&source).unwrap();
        assert_eq!(result["text"].clone().into_string().unwrap(), "abcd");
        assert!(result["truncated"].as_bool().unwrap());
        assert!(!result["complete"].as_bool().unwrap());
        server.join().unwrap();

        let error = engine()
            .eval::<Dynamic>(r#"rhai::http::request("GET", "file:///secret", #{proxy: false})"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("http_url_invalid"));

        let error = engine()
            .eval::<Dynamic>(
                r#"
                    rhai::http::request("GET", "http://127.0.0.1:1", #{
                        proxy: false,
                        timeout: std::time::Duration::from_millis(0)
                    })
                "#,
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("http_timeout_option"));
        assert!(error.contains("greater than zero"));
    }

    #[test]
    fn tls_handshake_failure_is_typed_and_bounded() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut client_hello = [0_u8; 1024];
            let bytes = stream.read(&mut client_hello).unwrap();
            assert!(bytes > 0);
            stream
                .write_all(
                    b"HTTP/1.1 400 Bad Request\r\n\
                      Connection: close\r\nContent-Length: 0\r\n\r\n",
                )
                .unwrap();
        });
        let started = std::time::Instant::now();
        let error = engine()
            .eval::<Dynamic>(&format!(
                r#"
                    rhai::http::request("GET", "https://{address}/tls", #{{
                        proxy: false,
                        timeout: std::time::Duration::from_millis(500)
                    }})
                "#
            ))
            .unwrap_err()
            .to_string();
        server.join().unwrap();
        assert!(error.contains("http_tls"), "{error}");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "TLS failure ignored the request deadline"
        );
    }
}
