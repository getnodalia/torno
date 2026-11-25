//! The network half of the update check and the package download (docs/updates.md), kept apart
//! from `main` so what decides each step can be tested without a network.
//!
//! - GitHub answers 60 anonymous API requests an hour per address, shared by everything on the
//!   network. When the check is refused (`403`/`429`), [`latest_from_release_page`] still learns
//!   the newest version from `github.com/<repo>/releases/latest`, a web redirect that is not
//!   counted, and offers that release with its page only.
//! - A package download has no deadline for the whole body, which a large file on a slow link
//!   outlasts. Every wait is bounded instead, and a connection that drops or stalls is picked up
//!   again with a `Range` request for what is missing.

use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use torno_ui_egui::update::install::MAX_PACKAGE;
use torno_ui_egui::update::{FetchError, RELEASES_PAGE};

const USER_AGENT: &str = "Torno-update-check";

/// The release page of a tag: `RELEASES_PAGE` without `latest`, then `tag/<tag>`.
const TAG_PAGE_PREFIX: &str = "https://github.com/getnodalia/torno/releases/tag/";

/// How long a download may go without receiving a byte before the attempt gives up.
const STALL: Duration = Duration::from_secs(60);

/// Attempts in a row that may end without a new byte before the download gives up.
const ATTEMPTS: u32 = 5;

fn tls() -> ureq::tls::TlsConfig {
    ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(std::sync::Arc::new(rustls::crypto::ring::default_provider()))
        .build()
}

/// Whether the update check was refused for a reason the release page can still answer: GitHub's
/// rate limit, or the plain `403` it sends when an address has spent it.
pub fn is_refusal(error: &FetchError) -> bool {
    match error {
        FetchError::RateLimited { .. } => true,
        FetchError::Failed(message) => message.ends_with("HTTP 403") || message.ends_with("HTTP 429"),
    }
}

/// `https://github.com/getnodalia/torno/releases/tag/v1.2.3` → `v1.2.3`. The redirect of a
/// repository with no release goes to `/releases`, which names no tag.
pub fn tag_from_page(page: &str) -> Option<String> {
    let tag = page.strip_prefix(TAG_PAGE_PREFIX)?;
    let tag = tag.split(['?', '#']).next().unwrap_or_default();
    let ok = !tag.is_empty() && tag.len() <= 100 && tag.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'));
    ok.then(|| tag.to_owned())
}

/// A release answer holding only what the release page tells: the tag and its page. It has no
/// assets, so Download opens the page.
pub fn release_json(tag: &str) -> Vec<u8> {
    serde_json::json!({ "tag_name": tag, "html_url": format!("{TAG_PAGE_PREFIX}{tag}") }).to_string().into_bytes()
}

/// The newest stable release as an answer `Release::parse` reads, learned from the release page
/// instead of the API. `None` when the page doesn't say.
pub fn latest_from_release_page() -> Option<Vec<u8>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls())
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .build()
        .into();
    let response = agent.head(RELEASES_PAGE).header("User-Agent", USER_AGENT).call().ok()?;
    if !response.status().is_redirection() {
        return None;
    }
    let location = response.headers().get("location")?.to_str().ok()?;
    tag_from_page(location).map(|tag| release_json(&tag))
}

/// `bytes <start>-<end>/<whole>` → `(start, whole)`; the whole is `None` for `*`.
pub fn content_range(value: &str) -> Option<(u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes ")?;
    let (span, whole) = rest.split_once('/')?;
    let (start, _end) = span.split_once('-')?;
    let whole = match whole.trim() {
        "*" => None,
        n => Some(n.parse().ok()?),
    };
    Some((start.trim().parse().ok()?, whole))
}

/// The wait before attempt `failures + 1`: 1, 2, 4, 8 seconds.
fn backoff(failures: u32) -> Duration {
    Duration::from_secs(1u64 << failures.saturating_sub(1).min(3))
}

/// How one attempt ended, when it didn't finish the file.
enum Attempt {
    /// Worth another try: the connection dropped or stalled.
    Retry(FetchError),
    /// Not worth it: GitHub refused, the file can't be written, the caller cancelled.
    Stop(FetchError),
}

/// Downloads `url` into `to`, reporting progress, picking up after a dropped or stalled
/// connection with a `Range` request. Stops when `progress` returns false. The caller verifies the
/// file.
pub fn download(url: &str, to: &Path, progress: &dyn Fn(u64, Option<u64>) -> bool) -> Result<(), FetchError> {
    let fail = |e: String| FetchError::Failed(e);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls())
        .https_only(true)
        .http_status_as_error(false)
        .max_redirects(3)
        .user_agent(USER_AGENT)
        // No deadline for the whole body; every wait for the server is bounded instead.
        .timeout_resolve(Some(Duration::from_secs(15)))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_per_read(Some(STALL))
        .build()
        .into();
    let mut file = std::fs::File::create(to).map_err(|e| fail(format!("{}: {e}", to.display())))?;
    let (mut done, mut total, mut failures) = (0u64, None, 0u32);
    loop {
        let before = done;
        match attempt(&agent, url, &mut file, &mut done, &mut total, progress) {
            Ok(()) => break,
            Err(Attempt::Stop(error)) => return Err(error),
            Err(Attempt::Retry(error)) => {
                failures = if done > before { 1 } else { failures + 1 };
                if failures >= ATTEMPTS {
                    return Err(error);
                }
                std::thread::sleep(backoff(failures));
            }
        }
    }
    file.sync_all().map_err(|e| fail(format!("{}: {e}", to.display())))
}

/// One request: the whole file, or what is missing after `done` bytes.
fn attempt(
    agent: &ureq::Agent,
    url: &str,
    file: &mut std::fs::File,
    done: &mut u64,
    total: &mut Option<u64>,
    progress: &dyn Fn(u64, Option<u64>) -> bool,
) -> Result<(), Attempt> {
    let interrupted = |e: String| Attempt::Retry(FetchError::Failed(e));
    let stop = |e: String| Attempt::Stop(FetchError::Failed(e));
    let mut request = agent.get(url);
    if *done > 0 {
        request = request.header("Range", format!("bytes={done}-"));
    }
    let mut response = request.call().map_err(|e| interrupted(e.to_string()))?;
    let status = response.status().as_u16();
    let length = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    match status {
        206 if *done > 0 => {
            let range = response.headers().get("content-range").and_then(|v| v.to_str().ok()).and_then(content_range);
            match range {
                Some((start, whole)) if start == *done => *total = whole.or(*total),
                // Not the bytes asked for: start over rather than splice the wrong ones in.
                _ => {
                    restart(file, done).map_err(stop)?;
                    return Err(interrupted("the server resumed at the wrong place".into()));
                }
            }
        }
        200 => {
            // A server that ignores the range sends the whole file again.
            if *done > 0 {
                restart(file, done).map_err(stop)?;
            }
            *total = length;
        }
        403 | 429 => return Err(stop(format!("GitHub answered HTTP {status}"))),
        // The file is complete already: what was asked starts past its end.
        416 if total.is_some_and(|t| *done == t) => return Ok(()),
        500..=599 => return Err(interrupted(format!("GitHub answered HTTP {status}"))),
        _ => return Err(stop(format!("GitHub answered HTTP {status}"))),
    }
    let mut body = response.body_mut().with_config().limit(MAX_PACKAGE.saturating_sub(*done)).reader();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = body.read(&mut buf).map_err(|e| interrupted(e.to_string()))?;
        if n == 0 {
            break;
        }
        file.write_all(buf.get(..n).unwrap_or_default()).map_err(|e| stop(e.to_string()))?;
        *done += n as u64;
        if !progress(*done, *total) {
            return Err(stop("cancelled".into()));
        }
    }
    match *total {
        Some(t) if *done < t => Err(interrupted("the connection closed before the whole file arrived".into())),
        _ => Ok(()),
    }
}

/// Empties the file to download it from its first byte again.
fn restart(file: &mut std::fs::File, done: &mut u64) -> Result<(), String> {
    use std::io::Seek;
    file.set_len(0).map_err(|e| e.to_string())?;
    file.rewind().map_err(|e| e.to_string())?;
    *done = 0;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tag_comes_from_this_repository_s_release_page_only() {
        assert_eq!(tag_from_page("https://github.com/getnodalia/torno/releases/tag/v0.1.1"), Some("v0.1.1".into()));
        assert_eq!(tag_from_page("https://github.com/getnodalia/torno/releases/tag/v0.1.1?x=1"), Some("v0.1.1".into()));
        for page in [
            "https://github.com/getnodalia/torno/releases",
            "https://github.com/getnodalia/other/releases/tag/v9.0.0",
            "https://github.com/getnodalia/torno/releases/tag/",
            "https://github.com/getnodalia/torno/releases/tag/v1/../../x",
            "http://github.com/getnodalia/torno/releases/tag/v1.0.0",
        ] {
            assert_eq!(tag_from_page(page), None, "{page}");
        }
    }

    #[test]
    fn the_page_answer_reads_as_a_release_without_packages() {
        let release = torno_ui_egui::update::Release::parse(&release_json("v0.1.2")).expect("a release");
        assert_eq!(release.version(), "0.1.2");
        assert!(release.assets.is_empty(), "the page names no packages");
        assert_eq!(release.page, "https://github.com/getnodalia/torno/releases/tag/v0.1.2");
    }

    #[test]
    fn only_a_refusal_falls_back_to_the_release_page() {
        assert!(is_refusal(&FetchError::RateLimited { retry_after_s: 60 }));
        assert!(is_refusal(&FetchError::Failed("GitHub answered HTTP 403".into())));
        assert!(is_refusal(&FetchError::Failed("GitHub answered HTTP 429".into())));
        assert!(!is_refusal(&FetchError::Failed("GitHub answered HTTP 500".into())));
        assert!(!is_refusal(&FetchError::Failed("dns error".into())));
    }

    #[test]
    fn a_content_range_is_read_or_refused() {
        assert_eq!(content_range("bytes 100-199/200"), Some((100, Some(200))));
        assert_eq!(content_range("bytes 0-9/*"), Some((0, None)));
        for bad in ["", "bytes", "bytes 1-2", "items 1-2/3", "bytes x-2/3", "bytes 1-2/y"] {
            assert_eq!(content_range(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_wait_between_attempts_grows_then_stops_growing() {
        let secs: Vec<u64> = (1..=6).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 8, 8]);
    }

    /// A one-connection-at-a-time HTTP server on this computer: the first request gets half of
    /// `body` and a dropped connection, the next gets the rest as `206` for the range asked.
    #[test]
    fn a_dropped_download_resumes_where_it_stopped() {
        use std::io::{BufRead, BufReader};
        use std::net::TcpListener;
        let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let served = body.clone();
        let server = std::thread::spawn(move || {
            for round in 0..2 {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut range = None;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).expect("line");
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        range = v.trim().trim_end_matches('-').parse::<usize>().ok();
                    }
                }
                let start = range.unwrap_or(0);
                let head = if range.is_some() {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{}\r\nContent-Length: {}\r\n\r\n",
                        served.len() - 1,
                        served.len(),
                        served.len() - start
                    )
                } else {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", served.len())
                };
                stream.write_all(head.as_bytes()).expect("head");
                let end = if round == 0 { served.len() / 2 } else { served.len() };
                stream.write_all(&served[start..end]).expect("body");
            }
        });
        let dir = std::env::temp_dir().join(format!("torno-update-http-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let to = dir.join("package");
        let agent: ureq::Agent =
            ureq::Agent::config_builder().http_status_as_error(false).timeout_per_read(Some(Duration::from_secs(10))).build().into();
        let mut file = std::fs::File::create(&to).expect("file");
        let (mut done, mut total) = (0u64, None);
        let url = format!("http://127.0.0.1:{port}/package");
        let first = attempt(&agent, &url, &mut file, &mut done, &mut total, &|_, _| true);
        assert!(matches!(first, Err(Attempt::Retry(_))), "the dropped connection is worth another try");
        assert_eq!(done, 100_000, "half arrived");
        attempt(&agent, &url, &mut file, &mut done, &mut total, &|_, _| true).map_err(|_| "second attempt").expect("resumed");
        drop(file);
        server.join().expect("server");
        assert_eq!(std::fs::read(&to).expect("read"), body, "the two halves make the whole file");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
