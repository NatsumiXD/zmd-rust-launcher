//! Windows transport and Authenticode verification; never executes downloaded code.
use crate::acquisition::{Backend, PUBLISHER, copy_exact, download_target};
use std::ffi::c_void;
use std::io::{self, Read, Write};
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use windows_sys::Win32::Networking::WinHttp::*;
use windows_sys::Win32::Security::Cryptography::{
    CERT_NAME_SIMPLE_DISPLAY_TYPE, CertGetNameStringW,
};
use windows_sys::Win32::Security::WinTrust::*;
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
};

struct Internet(*mut c_void);
impl Drop for Internet {
    fn drop(&mut self) {
        // SAFETY: each non-null WinHTTP handle is exclusively owned.
        unsafe { WinHttpCloseHandle(self.0) };
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn checked(handle: *mut c_void) -> Result<Internet, String> {
    if handle.is_null() {
        Err(format!("WinHTTP: {}", io::Error::last_os_error()))
    } else {
        Ok(Internet(handle))
    }
}
fn success(value: i32) -> Result<(), String> {
    if value == 0 {
        Err(format!("WinHTTP: {}", io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

pub struct NativeBackend;
struct Response {
    request: Internet,
    _connection: Internet,
    _session: Internet,
    deadline: Instant,
}
impl Response {
    fn get(host: &str, path: &str) -> Result<Self, String> {
        let agent = wide("EndfieldDirectLaunch/0.2");
        let host = wide(host);
        let path = wide(path);
        let method = wide("GET");
        // SAFETY: all UTF-16 buffers are NUL terminated and live through the calls;
        // handles stay owned for the whole synchronous request/response lifetime.
        unsafe {
            let session = checked(WinHttpOpen(
                agent.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                null(),
                null(),
                0,
            ))?;
            success(WinHttpSetTimeouts(session.0, 5000, 10000, 10000, 30000))?;
            let connection = checked(WinHttpConnect(session.0, host.as_ptr(), 443, 0))?;
            let request = checked(WinHttpOpenRequest(
                connection.0,
                method.as_ptr(),
                path.as_ptr(),
                null(),
                null(),
                null(),
                WINHTTP_FLAG_SECURE,
            ))?;
            let redirects = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
            success(WinHttpSetOption(
                request.0,
                WINHTTP_OPTION_REDIRECT_POLICY,
                (&redirects as *const u32).cast(),
                4,
            ))?;
            let disable = WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION;
            success(WinHttpSetOption(
                request.0,
                WINHTTP_OPTION_DISABLE_FEATURE,
                (&disable as *const u32).cast(),
                4,
            ))?;
            success(WinHttpSendRequest(request.0, null(), 0, null(), 0, 0, 0))?;
            success(WinHttpReceiveResponse(request.0, null_mut()))?;
            let mut status = 0u32;
            let mut length = 4u32;
            success(WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                null(),
                (&mut status as *mut u32).cast(),
                &mut length,
                null_mut(),
            ))?;
            if status != 200 {
                return Err(format!("official service returned HTTP {status}"));
            }
            Ok(Self {
                request,
                _connection: connection,
                _session: session,
                deadline: Instant::now() + Duration::from_secs(180),
            })
        }
    }
}
impl Read for Response {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if Instant::now() > self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "official download deadline exceeded",
            ));
        }
        if buffer.is_empty() {
            return Ok(0);
        }
        let mut read = 0;
        // SAFETY: buffer is writable for the requested size; request remains alive.
        if unsafe {
            WinHttpReadData(
                self.request.0,
                buffer.as_mut_ptr().cast(),
                buffer.len().min(u32::MAX as usize) as u32,
                &mut read,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(read as usize)
    }
}
impl Backend for NativeBackend {
    fn metadata(&mut self, version: &str) -> Result<Vec<u8>, String> {
        let path = format!(
            "/api/launcher/get_latest?appcode=abYeZZ16BPluCFyT&channel=1&sub_channel=1&version={version}&target_app=EndField"
        );
        let mut response = Response::get("launcher.hypergryph.com", &path)?;
        let mut bytes = Vec::new();
        response
            .by_ref()
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("metadata read failed: {e}"))?;
        if bytes.len() > 1_048_576 {
            return Err("launcher metadata exceeds 1 MiB".into());
        }
        Ok(bytes)
    }
    fn download(&mut self, url: &str, output: &mut dyn Write, size: u64) -> Result<(), String> {
        let (host, path) = download_target(url)?;
        copy_exact(&mut Response::get(host, path)?, output, size)
    }
    fn verify(&mut self, path: &Path) -> Result<String, String> {
        verify_official(path)
    }
    fn image_version(&mut self, path: &Path) -> Result<String, String> {
        let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: version.dll owns the parser; the aligned buffer and output
        // pointer remain alive, and fixed-info size is checked before reading.
        unsafe {
            let size = GetFileVersionInfoSizeW(name.as_ptr(), null_mut());
            if size == 0 || size > 1_048_576 {
                return Err("cannot read Games.exe file version".into());
            }
            let mut data = vec![0u64; (size as usize).div_ceil(8)];
            if GetFileVersionInfoW(name.as_ptr(), 0, size, data.as_mut_ptr().cast()) == 0 {
                return Err("cannot load Games.exe version resource".into());
            }
            let root = wide("\\");
            let mut info = null_mut();
            let mut len = 0;
            if VerQueryValueW(data.as_ptr().cast(), root.as_ptr(), &mut info, &mut len) == 0
                || info.is_null()
                || len < size_of::<VS_FIXEDFILEINFO>() as u32
            {
                return Err("missing fixed Games.exe version".into());
            }
            let value = std::ptr::read_unaligned(info.cast::<VS_FIXEDFILEINFO>());
            if value.dwSignature != 0xfeef04bd {
                return Err("invalid Games.exe version resource".into());
            }
            Ok(format!(
                "{}.{}.{}",
                value.dwFileVersionMS >> 16,
                value.dwFileVersionMS & 0xffff,
                value.dwFileVersionLS >> 16
            ))
        }
    }
}

struct TrustState(WINTRUST_DATA);
impl Drop for TrustState {
    fn drop(&mut self) {
        self.0.dwStateAction = WTD_STATEACTION_CLOSE;
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        // SAFETY: closes the state produced by this exact WinVerifyTrust request.
        unsafe {
            WinVerifyTrust(
                null_mut(),
                &mut action,
                (&mut self.0 as *mut WINTRUST_DATA).cast(),
            )
        };
    }
}
pub fn verify_official(path: &Path) -> Result<String, String> {
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: name.as_ptr(),
        ..Default::default()
    };
    let mut state = TrustState(WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &mut file_info,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        // Use OS trust/timestamp policy without UI or network revocation retrieval.
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_NONE,
        ..Default::default()
    });
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: structures and UTF-16 path remain alive until state is closed;
    // returned signer/certificate pointers are used only within this state.
    unsafe {
        let code = WinVerifyTrust(
            null_mut(),
            &mut action,
            (&mut state.0 as *mut WINTRUST_DATA).cast(),
        );
        if code != 0 {
            return Err(format!(
                "Games.exe Authenticode verification failed ({code:#x})"
            ));
        }
        let provider = WTHelperProvDataFromStateData(state.0.hWVTStateData);
        if provider.is_null() {
            return Err("missing verified signature provider".into());
        }
        let signer = WTHelperGetProvSignerFromChain(provider, 0, 0, 0);
        if signer.is_null() {
            return Err("missing verified signer".into());
        }
        let cert = WTHelperGetProvCertFromChain(signer, 0);
        if cert.is_null() || (*cert).pCert.is_null() {
            return Err("missing verified signer certificate".into());
        }
        let count = CertGetNameStringW(
            (*cert).pCert,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            null(),
            null_mut(),
            0,
        );
        if !(2..=4096).contains(&count) {
            return Err("invalid verified publisher name".into());
        }
        let mut text = vec![0u16; count as usize];
        if CertGetNameStringW(
            (*cert).pCert,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            null(),
            text.as_mut_ptr(),
            count,
        ) != count
        {
            return Err("cannot read verified publisher name".into());
        }
        let publisher = String::from_utf16(&text[..text.len() - 1])
            .map_err(|_| "invalid publisher encoding")?;
        if publisher != PUBLISHER {
            return Err("Games.exe has an unexpected signed publisher".into());
        }
        Ok(publisher)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn unsigned_test_executable_is_rejected_by_windows_trust() {
        assert!(super::verify_official(&std::env::current_exe().unwrap()).is_err());
    }
}
