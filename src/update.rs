//! "Check for updates": asks GitHub for the latest release of this project, and on request
//! downloads its setup, verifies it against the release's SHA256SUMS.txt and starts it.
//! Uses WinHTTP (part of Windows, loaded only when the user checks) and BCrypt for SHA-256.
use std::ffi::c_void;
use std::path::PathBuf;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};

/// GitHub repository the updates come from (owner/name).
pub const REPO: &str = "misiukasalgirdas-code/Wireless-Device-Battery-Monitor";
/// Prefix of the setup file in a release ("BatteryHub-Taskbar-Setup-v1.1.exe").
const SETUP_PREFIX: &str = "BatteryHub-Taskbar-Setup-";

#[derive(Clone, Debug)]
pub struct Release {
    /// "1.1" (tag without the leading v)
    pub version: String,
    pub setup_name: String,
    pub setup_url: String,
    pub sums_url: Option<String>,
    pub page_url: String,
}

#[derive(Clone, Debug)]
pub enum Check {
    UpToDate { latest: String },
    Available(Release),
    Failed(String),
}

/// Asks GitHub for the latest release and compares it with this build. Blocking (a second or so).
pub fn check() -> Check {
    match latest_release() {
        Ok(r) if crate::install::version_cmp(&r.version, crate::install::VERSION).is_gt() => Check::Available(r),
        Ok(r) => Check::UpToDate { latest: r.version },
        Err(e) => Check::Failed(e),
    }
}

fn latest_release() -> Result<Release, String> {
    let body = http_get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))?;
    parse_release(&String::from_utf8_lossy(&body)).ok_or_else(|| "unexpected answer from GitHub".into())
}

/// Picks the fields needed from GitHub's release JSON (a tiny scanner instead of a JSON library,
/// which would make the program several times larger).
fn parse_release(json: &str) -> Option<Release> {
    let tag = json_string(json, "tag_name")?;
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    let page_url = json_string(json, "html_url").unwrap_or_else(|| format!("https://github.com/{REPO}/releases/latest"));
    let urls = json_strings(json, "browser_download_url");
    let setup_url = urls.iter().find(|u| file_name(u).starts_with(SETUP_PREFIX) && u.ends_with(".exe"))?.clone();
    let sums_url = urls.iter().find(|u| file_name(u) == "SHA256SUMS.txt").cloned();
    Some(Release { version, setup_name: file_name(&setup_url).to_string(), setup_url, sums_url, page_url })
}

fn file_name(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or(url)
}

/// All string values of `"key": "value"` in `json` (no escapes are expected in these values).
fn json_strings(json: &str, key: &str) -> Vec<String> {
    let pat = format!("\"{key}\"");
    let mut out = Vec::new();
    let mut rest = json;
    while let Some(i) = rest.find(&pat) {
        rest = &rest[i + pat.len()..];
        let after = rest.trim_start();
        let Some(after) = after.strip_prefix(':') else { continue };
        let after = after.trim_start();
        let Some(after) = after.strip_prefix('"') else { continue };
        if let Some(end) = after.find('"') {
            out.push(after[..end].to_string());
        }
    }
    out
}

fn json_string(json: &str, key: &str) -> Option<String> {
    json_strings(json, key).into_iter().next()
}

/// Downloads the setup of `r` to %TEMP%\BatteryHub-Update, checks its SHA-256 against the
/// release's SHA256SUMS.txt (when the release has one) and returns its path.
pub fn download(r: &Release) -> Result<PathBuf, String> {
    let data = http_get(&r.setup_url)?;
    if let Some(sums_url) = &r.sums_url {
        let sums = String::from_utf8_lossy(&http_get(sums_url)?).to_string();
        let expected = sums
            .lines()
            .find(|l| l.split_whitespace().nth(1) == Some(r.setup_name.as_str()))
            .and_then(|l| l.split_whitespace().next())
            .ok_or("the release's SHA256SUMS.txt does not list the setup")?
            .to_lowercase();
        let actual = sha256_hex(&data)?;
        if actual != expected {
            return Err(format!("checksum mismatch (expected {expected}, got {actual}) - the download was not started"));
        }
    }
    let dir = std::env::temp_dir().join("BatteryHub-Update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&r.setup_name);
    std::fs::write(&path, &data).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn sha256_hex(data: &[u8]) -> Result<String, String> {
    let mut hash = [0u8; 32];
    let st = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut hash) };
    if st.is_err() {
        return Err("SHA-256 failed".into());
    }
    Ok(hash.iter().map(|b| format!("{b:02x}")).collect())
}

// ---- WinHTTP -----------------------------------------------------------------------------------

struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// HTTPS GET (redirects followed, e.g. to GitHub's download servers). Errors on non-200 answers.
pub fn http_get(url: &str) -> Result<Vec<u8>, String> {
    let rest = url.strip_prefix("https://").ok_or("only https URLs are used")?;
    let (host, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
    unsafe {
        let session = Handle(WinHttpOpen(
            &HSTRING::from(format!("BatteryHub/{}", crate::install::VERSION)),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ));
        if session.0.is_null() {
            return Err("WinHTTP is not available".into());
        }
        let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 30_000);
        let connect = Handle(WinHttpConnect(session.0, &HSTRING::from(host), 443, 0));
        if connect.0.is_null() {
            return Err(format!("cannot reach {host}"));
        }
        let request = Handle(WinHttpOpenRequest(
            connect.0,
            &HSTRING::from("GET"),
            &HSTRING::from(path.as_str()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return Err("cannot create the request".into());
        }
        let headers: Vec<u16> = "Accept: application/vnd.github+json\r\n".encode_utf16().collect();
        let _ = WinHttpAddRequestHeaders(request.0, &headers, WINHTTP_ADDREQ_FLAG_ADD);
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|e| format!("no connection ({})", e.message()))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| format!("no answer ({})", e.message()))?;
        let mut status = 0u32;
        let mut len = 4u32;
        let mut index = 0u32;
        let _ = WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut len,
            &mut index,
        );
        if status != 200 {
            return Err(if status == 404 { "no release found".into() } else { format!("HTTP {status}") });
        }
        let mut body = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(request.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut read).map_err(|e| e.message().to_string())?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buf[..read as usize]);
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "html_url": "https://github.com/ferloom/Wireless-Device-Battery-Monitor/releases/tag/v1.5",
      "tag_name": "v1.5",
      "assets": [
        { "name": "BatteryHub-Taskbar-Setup-v1.5.exe",
          "browser_download_url": "https://github.com/ferloom/Wireless-Device-Battery-Monitor/releases/download/v1.5/BatteryHub-Taskbar-Setup-v1.5.exe" },
        { "name": "SHA256SUMS.txt",
          "browser_download_url": "https://github.com/ferloom/Wireless-Device-Battery-Monitor/releases/download/v1.5/SHA256SUMS.txt" }
      ]
    }"#;

    #[test]
    fn parses_a_github_release() {
        let r = parse_release(SAMPLE).unwrap();
        assert_eq!(r.version, "1.5");
        assert_eq!(r.setup_name, "BatteryHub-Taskbar-Setup-v1.5.exe");
        assert!(r.sums_url.unwrap().ends_with("/SHA256SUMS.txt"));
        assert!(r.page_url.ends_with("/tag/v1.5"));
    }

    #[test]
    fn release_without_a_setup_is_rejected() {
        assert!(parse_release(r#"{"tag_name": "v2.0", "assets": []}"#).is_none());
    }

    #[test]
    fn sha256_known_value() {
        assert_eq!(sha256_hex(b"abc").unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn newer_versions_compare_correctly() {
        use crate::install::version_cmp;
        assert!(version_cmp("1.1", "1.0.1.1").is_gt());
        assert!(version_cmp("1.0.1.1", "1.0.1").is_gt());
        assert!(version_cmp("1.5", "1.10").is_lt());
    }
}
