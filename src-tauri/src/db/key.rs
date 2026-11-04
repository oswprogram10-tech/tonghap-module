//! 기록 DB 암호화 키: 32바이트 난수를 `data/db.key`에 보관한다. 설계: docs/design/20-db-encryption.md 1절.
//!
//! 파일 형식은 앞머리로 구분한다.
//! - `DPAPI1` + `CryptProtectData`로 보호한 바이트: 윈도우(현재 사용자 범위, 앱 식별자를 추가 엔트로피로 사용)
//! - `PLAIN1` + 원본 키 32바이트: 윈도우 밖(개발·리눅스 CI). 윈도우 빌드는 이 형식을 거부한다.
//!
//! 키 값은 로그·오류 문구에 남기지 않는다(`DbKey`의 `Debug`도 값을 감춘다).

use std::path::Path;

use super::DbResult;

pub const KEY_FILE: &str = "db.key";
const HEAD_DPAPI: &[u8] = b"DPAPI1";
const HEAD_PLAIN: &[u8] = b"PLAIN1";
pub const KEY_LEN: usize = 32;
/// DPAPI 추가 엔트로피(앱 식별자). `tauri.conf.json`의 identifier와 같다.
pub const APP_ENTROPY: &[u8] = b"io.github.oswprogram10-tech.tonghap";

#[derive(Clone, PartialEq, Eq)]
pub struct DbKey([u8; KEY_LEN]);

impl std::fmt::Debug for DbKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DbKey(가림)")
    }
}

impl DbKey {
    pub fn generate() -> DbResult<Self> {
        let mut raw = [0u8; KEY_LEN];
        getrandom::fill(&mut raw).map_err(|_| "암호화 키를 만들 수 없습니다(난수를 얻지 못했습니다)".to_string())?;
        Ok(Self(raw))
    }

    #[cfg(test)]
    pub fn from_bytes(raw: [u8; KEY_LEN]) -> Self {
        Self(raw)
    }

    /// `PRAGMA key`에 넘기는 원본 키 표기(`x'<64 hex>'`). 문자열 키 유도를 거치지 않는다.
    pub fn sql_literal(&self) -> String {
        format!("x'{}'", hex::encode(self.0))
    }
}

// ---------- 보호(윈도우 DPAPI / 그 밖) ----------

#[cfg(windows)]
mod os {
    use super::*;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
    }

    /// 시스템이 할당한 출력 블롭을 복사해 가져오고 `LocalFree`로 돌려준다.
    ///
    /// # Safety
    /// `out`은 `CryptProtectData`/`CryptUnprotectData`가 성공한 뒤의 값이어야 한다.
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let bytes = if out.pbData.is_null() { Vec::new() } else { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
        if !out.pbData.is_null() {
            LocalFree(out.pbData as *mut core::ffi::c_void);
        }
        bytes
    }

    pub fn protect(raw: &[u8], entropy: &[u8]) -> DbResult<Vec<u8>> {
        let input = blob(raw);
        let ent = blob(entropy);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: 입력·엔트로피 블롭은 이 호출 동안 살아 있는 슬라이스를 가리키고, 출력은 시스템이 할당해 `take`가 해제한다.
        let ok = unsafe { CryptProtectData(&input, std::ptr::null(), &ent, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) };
        if ok == 0 {
            return Err("윈도우 계정으로 키를 보호하지 못했습니다".to_string());
        }
        Ok(unsafe { take(out) })
    }

    pub fn unprotect(sealed: &[u8], entropy: &[u8]) -> DbResult<Vec<u8>> {
        let input = blob(sealed);
        let ent = blob(entropy);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: 위와 같다. 설명 문자열 출력은 쓰지 않으므로 null을 넘긴다.
        let ok = unsafe { CryptUnprotectData(&input, std::ptr::null_mut(), &ent, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) };
        if ok == 0 {
            return Err("이 윈도우 계정으로는 키를 풀 수 없습니다".to_string());
        }
        Ok(unsafe { take(out) })
    }
}

/// 키 파일에 쓸 바이트(앞머리 포함).
pub fn encode_key_file(key: &DbKey, entropy: &[u8]) -> DbResult<Vec<u8>> {
    #[cfg(windows)]
    {
        let mut out = HEAD_DPAPI.to_vec();
        out.extend(os::protect(&key.0, entropy)?);
        Ok(out)
    }
    #[cfg(not(windows))]
    {
        let _ = entropy;
        let mut out = HEAD_PLAIN.to_vec();
        out.extend_from_slice(&key.0);
        Ok(out)
    }
}

/// 키 파일 바이트에서 키를 꺼낸다. 형식이 다르거나 손상되었으면 오류.
pub fn decode_key_file(bytes: &[u8], entropy: &[u8]) -> DbResult<DbKey> {
    let bad = || "키 파일이 손상되었습니다".to_string();
    let raw: Vec<u8> = if let Some(rest) = bytes.strip_prefix(HEAD_DPAPI) {
        #[cfg(windows)]
        {
            os::unprotect(rest, entropy)?
        }
        #[cfg(not(windows))]
        {
            let _ = (rest, entropy);
            return Err("이 키 파일은 윈도우 계정으로 보호되어 있어 이 PC에서는 풀 수 없습니다".to_string());
        }
    } else if let Some(rest) = bytes.strip_prefix(HEAD_PLAIN) {
        #[cfg(windows)]
        {
            let _ = rest;
            return Err("보호되지 않은 형식의 키 파일은 윈도우에서 쓰지 않습니다".to_string());
        }
        #[cfg(not(windows))]
        {
            let _ = entropy;
            rest.to_vec()
        }
    } else {
        return Err(bad());
    };
    let arr: [u8; KEY_LEN] = raw.try_into().map_err(|_| bad())?;
    Ok(DbKey(arr))
}

// ---------- 파일 ----------

/// 키 파일 읽기 결과. `Unreadable`은 파일은 있으나 풀 수 없는 경우(다른 윈도우 계정, 손상 등).
#[derive(Debug)]
pub enum KeyFile {
    Missing,
    Key(DbKey),
    Unreadable(String),
}

pub fn read_key_file(data_dir: &Path) -> KeyFile {
    match std::fs::read(data_dir.join(KEY_FILE)) {
        Ok(bytes) => match decode_key_file(&bytes, APP_ENTROPY) {
            Ok(k) => KeyFile::Key(k),
            Err(e) => KeyFile::Unreadable(e),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => KeyFile::Missing,
        Err(e) => KeyFile::Unreadable(format!("키 파일을 읽을 수 없습니다: {e}")),
    }
}

/// 키 파일을 쓴다(임시 이름에 쓰고 이름을 바꾼다). 유닉스는 권한 0600.
pub fn write_key_file_at(path: &Path, key: &DbKey) -> DbResult<()> {
    let bytes = encode_key_file(key, APP_ENTROPY)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let err = |e: std::io::Error| format!("키 파일을 저장할 수 없습니다: {e}");
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(err)?;
        f.write_all(&bytes).map_err(err)?;
        f.sync_all().map_err(err)?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        err(e)
    })
}

pub fn write_key_file(data_dir: &Path, key: &DbKey) -> DbResult<()> {
    write_key_file_at(&data_dir.join(KEY_FILE), key)
}

/// 풀 수 없는 키 파일을 지우지 않고 옆으로 치워 둔다(`db.key.unreadable-<시각>`).
/// 예전 보관 폴더의 암호화 DB를 나중에 원래 계정에서 열 수 있도록 남긴다.
pub fn set_aside_key_file(data_dir: &Path) {
    let p = data_dir.join(KEY_FILE);
    if p.exists() {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let _ = std::fs::rename(&p, data_dir.join(format!("{KEY_FILE}.unreadable-{stamp}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("tonghap-key-{}", super::super::new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn debug_never_shows_the_key() {
        let k = DbKey::from_bytes([0xAB; 32]);
        assert!(!format!("{k:?}").contains("ab"));
        assert_eq!(k.sql_literal(), format!("x'{}'", "ab".repeat(32)));
    }

    #[test]
    fn generated_keys_differ() {
        assert_ne!(DbKey::generate().unwrap(), DbKey::generate().unwrap());
    }

    #[cfg(not(windows))]
    #[test]
    fn plain1_roundtrip_and_permissions() {
        let dir = tmp();
        let k = DbKey::generate().unwrap();
        write_key_file(&dir, &k).unwrap();
        let bytes = std::fs::read(dir.join(KEY_FILE)).unwrap();
        assert!(bytes.starts_with(b"PLAIN1"));
        match read_key_file(&dir) {
            KeyFile::Key(got) => assert_eq!(got, k),
            other => panic!("{other:?}"),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join(KEY_FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(!dir.join("db.key.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(windows))]
    #[test]
    fn dpapi_file_is_refused_off_windows() {
        assert!(decode_key_file(b"DPAPI1\x01\x02", APP_ENTROPY).is_err());
    }

    #[test]
    fn corrupt_key_files_are_errors() {
        let dir = tmp();
        assert!(matches!(read_key_file(&dir), KeyFile::Missing));
        for bad in [&b""[..], b"PLAIN1", b"PLAIN1short", b"XXXXXX0123456789012345678901234567890123", b"DPAPI1"] {
            std::fs::write(dir.join(KEY_FILE), bad).unwrap();
            assert!(matches!(read_key_file(&dir), KeyFile::Unreadable(_)), "{bad:?}");
        }
        // PLAIN1 뒤에 33바이트
        let mut long = b"PLAIN1".to_vec();
        long.extend([1u8; 33]);
        std::fs::write(dir.join(KEY_FILE), long).unwrap();
        assert!(matches!(read_key_file(&dir), KeyFile::Unreadable(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_aside_keeps_the_old_file() {
        let dir = tmp();
        std::fs::write(dir.join(KEY_FILE), b"junk").unwrap();
        set_aside_key_file(&dir);
        assert!(!dir.join(KEY_FILE).exists());
        let n = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("db.key.unreadable-")).count();
        assert_eq!(n, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip() {
        let dir = tmp();
        let k = DbKey::generate().unwrap();
        write_key_file(&dir, &k).unwrap();
        let bytes = std::fs::read(dir.join(KEY_FILE)).unwrap();
        assert!(bytes.starts_with(b"DPAPI1"));
        // 보호된 바이트에 원본 키가 그대로 들어 있지 않다.
        assert!(!bytes.windows(KEY_LEN).any(|w| w == k.0));
        match read_key_file(&dir) {
            KeyFile::Key(got) => assert_eq!(got, k),
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_with_other_entropy_cannot_unprotect() {
        let k = DbKey::generate().unwrap();
        let bytes = encode_key_file(&k, APP_ENTROPY).unwrap();
        assert!(decode_key_file(&bytes, b"other-entropy").is_err());
        assert_eq!(decode_key_file(&bytes, APP_ENTROPY).unwrap(), k);
    }

    #[cfg(windows)]
    #[test]
    fn plain1_file_is_refused_on_windows() {
        let mut bytes = b"PLAIN1".to_vec();
        bytes.extend([7u8; 32]);
        assert!(decode_key_file(&bytes, APP_ENTROPY).is_err());
    }
}
