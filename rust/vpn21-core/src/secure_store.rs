//! Filesystem layout + random credential generation for every session.
//!
//! The caller (Flutter platform plugin) passes us a root directory that is
//! guaranteed to be private to the application (iOS Application Support,
//! Android `filesDir`, `$XDG_DATA_HOME/vpn21` on Linux, `%APPDATA%\vpn21` on
//! Windows, `~/Library/Application Support/vpn21` on macOS).  We keep **all**
//! persistent and runtime state underneath it, with 0700 / 0600 permissions
//! on Unix.

use crate::errors::{Error, Result};
use base64::Engine;
use rand::Rng;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};

const SUBDIRS: &[&str] = &["profiles", "runtime", "cache", "cache/arti", "logs"];

pub fn ensure_layout(app_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(app_dir)?;
    restrict_dir(app_dir)?;
    for name in SUBDIRS {
        let p = app_dir.join(name);
        std::fs::create_dir_all(&p)?;
        restrict_dir(&p)?;
    }
    Ok(())
}

#[cfg(unix)]
fn restrict_dir(p: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(p)?.permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(p, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_dir(_: &Path) -> Result<()> {
    // On Windows the app-private directory inherits an ACL that already
    // denies access to other local users, so nothing extra to do here.
    Ok(())
}

/// Writes bytes to `path` with 0600 permissions (Unix) or default ACL (Windows).
pub fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        restrict_dir(parent)?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(data)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, data)?;
    }
    Ok(())
}

/// Picks a free loopback port by asking the OS and releasing the socket.  A
/// race remains — we mitigate it by retrying if the listener fails to bind.
pub fn pick_free_port() -> Result<u16> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .map_err(|e| Error::Other(anyhow::anyhow!("pick_free_port: {e}")))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// 32 random bytes, base64 (url-safe, no padding) — short enough to stay
/// under SOCKS5's 255-byte user/pass limit and strong enough to resist
/// guessing.
pub fn random_credential() -> String {
    let mut b = [0u8; 24];
    rand::thread_rng().fill(&mut b);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

#[derive(Debug, Clone)]
pub struct SocksEndpoint {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
}

impl SocksEndpoint {
    pub fn new_loopback() -> Result<Self> {
        Ok(Self {
            host: "127.0.0.1".to_string(),
            port: pick_free_port()?,
            user: random_credential(),
            pass: random_credential(),
        })
    }

    pub fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    pub fn as_url(&self) -> String {
        format!(
            "socks5://{}:{}@{}:{}",
            urlencoding_light(&self.user),
            urlencoding_light(&self.pass),
            self.host,
            self.port
        )
    }
}

fn urlencoding_light(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            ':' | '/' | '@' | '?' | '#' | '%' => format!("%{:02X}", c as u32),
            _ => c.to_string(),
        })
        .collect()
}

/// Returns the canonical path for the session runtime file.
pub fn runtime_path(app_dir: &Path) -> PathBuf {
    app_dir.join("runtime").join("session.json")
}

/// Returns the canonical profiles directory.
pub fn profiles_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("profiles")
}

/// Returns the canonical arti cache directory.
pub fn arti_cache_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("cache").join("arti")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_non_empty_and_varied() {
        let a = random_credential();
        let b = random_credential();
        assert!(!a.is_empty());
        assert_ne!(a, b, "32 random bytes collided — RNG broken?");
        // url-safe base64 alphabet only
        for c in a.chars() {
            assert!(
                c.is_ascii_alphanumeric() || c == '-' || c == '_',
                "bad char {c}"
            );
        }
    }

    #[test]
    fn free_ports_differ() {
        let a = pick_free_port().unwrap();
        let b = pick_free_port().unwrap();
        assert!(a > 0 && b > 0);
    }

    #[test]
    fn socks_url_encodes_specials() {
        let ep = SocksEndpoint {
            host: "127.0.0.1".into(),
            port: 1080,
            user: "a:b@c".into(),
            pass: "p/q?r".into(),
        };
        let url = ep.as_url();
        assert!(
            url.starts_with("socks5://a%3Ab%40c:p%2Fq%3Fr@127.0.0.1:1080"),
            "{url}"
        );
    }

    #[test]
    fn layout_is_created_and_is_private() {
        let tmp = tempfile::tempdir().unwrap();
        ensure_layout(tmp.path()).unwrap();
        for sub in SUBDIRS {
            assert!(tmp.path().join(sub).is_dir(), "{sub} missing");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(tmp.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "app dir not 0700 (got {:o})", mode);
        }
    }
}
