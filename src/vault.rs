//! 종단간 암호화 동기화 저장소(순수 로직). 전송은 `Remote` 트레이트 뒤에 숨긴다.
//!
//! 서버에는 vault-id(폴더 이름)와 암호문만 간다. 마스터 키·암호화 키는 밖으로 나가지 않는다.
//! 배치: `manifest`(암호화) + `<세대>.<번호>.part`(암호화된 512KB 조각).
//! 기존 폴더 방식(`sync.rs`)과는 별개이며, 512KB 조각·세대 이름 규칙만 같다.

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 조각 크기(기존 폴더 방식과 같은 512KB).
pub const CHUNK: usize = crate::sync::CHUNK;

/// 서버(폴더) 안의 manifest 파일 이름.
pub const MANIFEST: &str = "manifest";
/// 암호문 형식 버전(첫 바이트).
const FORMAT: u8 = 1;
const NONCE_LEN: usize = 24;
const RECOVERY_PREFIX: &str = "AKS1";

// ───────────────────────── 오류 ─────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum VaultError {
    /// 다른 기기가 먼저 올렸다(HTTP 412 / If-Match 불일치). 올리기를 중단하고 받아오기를 안내한다.
    Conflict,
    /// manifest 를 이 키로 복호화할 수 없다.
    WrongKey,
    /// 조각·manifest 가 손상되었거나 바뀌었다.
    Corrupt(String),
    /// 전송 계층 오류.
    Remote(String),
    /// 로컬 파일 오류.
    Io(String),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::Conflict => write!(f, "다른 기기가 먼저 올렸습니다. 먼저 받아오세요."),
            VaultError::WrongKey => write!(f, "키가 다릅니다."),
            VaultError::Corrupt(s) => write!(f, "저장소가 손상되었거나 바뀌었습니다: {s}"),
            VaultError::Remote(s) => write!(f, "서버 오류: {s}"),
            VaultError::Io(s) => write!(f, "파일 오류: {s}"),
        }
    }
}

impl std::error::Error for VaultError {}

impl From<RemoteError> for VaultError {
    fn from(e: RemoteError) -> Self {
        match e {
            RemoteError::Conflict => VaultError::Conflict,
            RemoteError::Other(s) => VaultError::Remote(s),
        }
    }
}

// ───────────────────────── Remote ─────────────────────────

/// 서버의 저장소 폴더 하나를 가리키는 전송 계층. 이름은 폴더 안의 상대 이름(경로 구분자 없음).
pub type Etag = String;

#[derive(Debug, Clone, PartialEq)]
pub enum RemoteError {
    /// 조건부 올리기 실패(412). 다른 오류와 구분해야 한다.
    Conflict,
    Other(String),
}

/// 올리기 조건.
#[derive(Debug, Clone, PartialEq)]
pub enum PutCond {
    /// 무조건 덮어씀(조각용).
    Any,
    /// `If-Match: <etag>` — 지금 서버의 ETag 가 같을 때만.
    IfMatch(Etag),
    /// `If-None-Match: *` — 파일이 아직 없을 때만.
    IfNoneMatch,
}

pub trait Remote {
    /// 파일을 받는다. 없으면 `Ok(None)`. ETag 를 같이 돌려준다.
    fn get(&self, name: &str) -> Result<Option<(Vec<u8>, Etag)>, RemoteError>;
    /// 파일을 올리고 새 ETag 를 돌려준다. 조건이 안 맞으면 `Err(Conflict)`.
    fn put(&self, name: &str, data: &[u8], cond: PutCond) -> Result<Etag, RemoteError>;
    /// 파일을 지운다. 없어도 성공.
    fn delete(&self, name: &str) -> Result<(), RemoteError>;
    /// 저장소 폴더 안의 파일 이름들. 폴더가 없으면 빈 목록.
    fn list(&self) -> Result<Vec<String>, RemoteError>;
}

fn bad_name(name: &str) -> bool {
    name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".."
}

/// 로컬 폴더를 서버처럼 쓰는 구현(기존 폴더 방식과 같은 곳을 가리킬 수 있다).
/// ETag 는 내용의 SHA-256 앞부분. 비교-후-쓰기는 한 프로세스 안에서만 원자적이다.
pub struct FolderRemote {
    dir: PathBuf,
}

impl FolderRemote {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        FolderRemote { dir: dir.into() }
    }

    fn etag_of(data: &[u8]) -> Etag {
        format!("\"{}\"", &hex(&Sha256::digest(data))[..32])
    }
}

fn tmp_suffix() -> &'static str {
    ".akashic-tmp"
}

impl Remote for FolderRemote {
    fn get(&self, name: &str) -> Result<Option<(Vec<u8>, Etag)>, RemoteError> {
        if bad_name(name) {
            return Err(RemoteError::Other(format!("잘못된 이름: {name}")));
        }
        match fs::read(self.dir.join(name)) {
            Ok(d) => {
                let e = Self::etag_of(&d);
                Ok(Some((d, e)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(RemoteError::Other(e.to_string())),
        }
    }

    fn put(&self, name: &str, data: &[u8], cond: PutCond) -> Result<Etag, RemoteError> {
        if bad_name(name) {
            return Err(RemoteError::Other(format!("잘못된 이름: {name}")));
        }
        let path = self.dir.join(name);
        let current = match fs::read(&path) {
            Ok(d) => Some(Self::etag_of(&d)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(RemoteError::Other(e.to_string())),
        };
        match (&cond, &current) {
            (PutCond::IfNoneMatch, Some(_)) => return Err(RemoteError::Conflict),
            (PutCond::IfMatch(want), cur) if cur.as_ref() != Some(want) => return Err(RemoteError::Conflict),
            _ => {}
        }
        fs::create_dir_all(&self.dir).map_err(|e| RemoteError::Other(e.to_string()))?;
        let tmp = self.dir.join(format!("{name}{}", tmp_suffix()));
        let res = (|| -> std::io::Result<()> {
            let mut f = File::create(&tmp)?;
            f.write_all(data)?;
            f.sync_all()?;
            drop(f);
            fs::rename(&tmp, &path)
        })();
        if let Err(e) = res {
            let _ = fs::remove_file(&tmp);
            return Err(RemoteError::Other(e.to_string()));
        }
        Ok(Self::etag_of(data))
    }

    fn delete(&self, name: &str) -> Result<(), RemoteError> {
        if bad_name(name) {
            return Err(RemoteError::Other(format!("잘못된 이름: {name}")));
        }
        match fs::remove_file(self.dir.join(name)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(RemoteError::Other(e.to_string())),
        }
    }

    fn list(&self) -> Result<Vec<String>, RemoteError> {
        let rd = match fs::read_dir(&self.dir) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(RemoteError::Other(e.to_string())),
        };
        let mut v: Vec<String> = rd
            .flatten()
            .filter(|e| e.file_type().map_or(false, |t| t.is_file()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.ends_with(tmp_suffix()))
            .collect();
        v.sort();
        Ok(v)
    }
}

// ───────────────────────── 키 ─────────────────────────

/// 사람이 읽기 쉬운 32문자 알파벳(I·O·0·1 제외).
const B32: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// 저장소 마스터 키(32바이트). 복구 문자열로 사람이 옮겨 적는다.
#[derive(Clone, PartialEq)]
pub struct VaultKey {
    master: [u8; 32],
}

impl fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VaultKey(..)") // 키가 로그에 새지 않게
    }
}

fn checksum(master: &[u8; 32]) -> [u8; 3] {
    let mut m = <Hmac<Sha256> as hmac::KeyInit>::new_from_slice(b"akashic-recovery-checksum").expect("HMAC 은 모든 키 길이를 받는다");
    m.update(RECOVERY_PREFIX.as_bytes());
    m.update(master);
    let t = m.finalize().into_bytes();
    [t[0], t[1], t[2]]
}

impl VaultKey {
    /// 무작위 새 키.
    pub fn generate() -> Result<VaultKey, String> {
        let mut master = [0u8; 32];
        getrandom::fill(&mut master).map_err(|e| format!("난수를 얻을 수 없음: {e}"))?;
        Ok(VaultKey { master })
    }

    pub fn from_bytes(master: [u8; 32]) -> VaultKey {
        VaultKey { master }
    }

    /// 복구 문자열: `AKS1-XXXX-XXXX-...`(키 32바이트 + 체크섬 3바이트를 base32 로, 4자씩 14묶음).
    pub fn recovery_string(&self) -> String {
        let mut bytes = Vec::with_capacity(35);
        bytes.extend_from_slice(&self.master);
        bytes.extend_from_slice(&checksum(&self.master));
        let mut chars = String::new();
        let (mut acc, mut bits) = (0u32, 0u32);
        for b in bytes {
            acc = (acc << 8) | b as u32;
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                chars.push(B32[((acc >> bits) & 31) as usize] as char);
            }
        }
        // 35바이트 = 280비트 = 정확히 56문자라 남는 비트가 없다.
        let mut out = String::from(RECOVERY_PREFIX);
        for c in chars.as_bytes().chunks(4) {
            out.push('-');
            out.push_str(std::str::from_utf8(c).unwrap());
        }
        out
    }

    /// 복구 문자열을 읽는다. 대소문자·공백·하이픈 위치는 신경 쓰지 않고, 체크섬이 틀리면 오류.
    pub fn from_recovery(s: &str) -> Result<VaultKey, String> {
        let up: String = s.chars().filter(|c| !c.is_whitespace() && *c != '-').collect::<String>().to_ascii_uppercase();
        let body = up.strip_prefix(RECOVERY_PREFIX).ok_or("복구 문자열은 AKS1 로 시작해야 합니다.")?;
        if body.len() != 56 {
            return Err("복구 문자열 길이가 맞지 않습니다.".into());
        }
        let mut bytes = Vec::with_capacity(35);
        let (mut acc, mut bits) = (0u32, 0u32);
        for c in body.bytes() {
            let v = B32.iter().position(|&x| x == c).ok_or_else(|| format!("복구 문자열에 쓸 수 없는 글자가 있습니다: {}", c as char))?;
            acc = (acc << 5) | v as u32;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                bytes.push(((acc >> bits) & 0xff) as u8);
            }
        }
        let mut master = [0u8; 32];
        master.copy_from_slice(&bytes[..32]);
        if checksum(&master)[..] != bytes[32..35] {
            return Err("복구 문자열이 틀렸습니다(확인 코드 불일치). 다시 확인하세요.".into());
        }
        Ok(VaultKey { master })
    }

    fn expand(&self, info: &[u8], out: &mut [u8]) {
        Hkdf::<Sha256>::new(Some(b"akashic-vault-v1"), &self.master).expand(info, out).expect("HKDF 출력 길이가 너무 길지 않다");
    }

    /// 서버 폴더 식별자(16바이트). AAD 에도 쓴다.
    fn vault_id_bytes(&self) -> [u8; 16] {
        let mut id = [0u8; 16];
        self.expand(b"vault-id", &mut id);
        id
    }

    /// 같은 키는 항상 같은 값(32자 hex).
    pub fn vault_id(&self) -> String {
        hex(&self.vault_id_bytes())
    }

    /// 서버 폴더 이름 `akashic-<hex>`.
    pub fn folder_name(&self) -> String {
        format!("akashic-{}", self.vault_id())
    }

    /// 본문 일부 암호화(textcrypt)용 키. 같은 저장소를 쓰는 PC들이 같은 키를 얻는다.
    pub fn text_key(&self) -> [u8; 32] {
        let mut k = [0u8; 32];
        self.expand(b"text", &mut k);
        k
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        let mut k = [0u8; 32];
        self.expand(b"enc", &mut k);
        XChaCha20Poly1305::new_from_slice(&k).expect("키 길이 32")
    }
}

// ───────────────────────── 암호화 ─────────────────────────

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sha_hex(d: &[u8]) -> String {
    hex(&Sha256::digest(d))
}

/// AAD: vault-id + 종류 + 세대 + 번호. 조각을 다른 자리로 옮기거나 다른 저장소로 옮기면 복호화가 실패한다.
fn aad(key: &VaultKey, kind: u8, generation: i64, index: u64) -> Vec<u8> {
    let mut a = Vec::with_capacity(16 + 1 + 8 + 8);
    a.extend_from_slice(&key.vault_id_bytes());
    a.push(kind);
    a.extend_from_slice(&generation.to_le_bytes());
    a.extend_from_slice(&index.to_le_bytes());
    a
}

const KIND_MANIFEST: u8 = b'M';
const KIND_PART: u8 = b'P';

/// `[형식 1바이트][nonce 24바이트][암호문+태그]`. nonce 는 호출마다 무작위.
fn seal(key: &VaultKey, aad: &[u8], plain: &[u8]) -> Result<Vec<u8>, VaultError> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|e| VaultError::Io(format!("난수를 얻을 수 없음: {e}")))?;
    let ct = key
        .cipher()
        .encrypt((&nonce).into(), Payload { msg: plain, aad })
        .map_err(|_| VaultError::Corrupt("암호화 실패".into()))?;
    let mut out = Vec::with_capacity(1 + NONCE_LEN + ct.len());
    out.push(FORMAT);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// 복호화. 실패하면 `None`(키가 틀렸거나 바뀐 것 — 호출한 쪽이 의미를 정한다).
fn open(key: &VaultKey, aad: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 1 + NONCE_LEN + 16 || data[0] != FORMAT {
        return None;
    }
    let nonce: [u8; NONCE_LEN] = data[1..1 + NONCE_LEN].try_into().ok()?;
    key.cipher().decrypt((&nonce).into(), Payload { msg: &data[1 + NONCE_LEN..], aad }).ok()
}

/// 문서 단위 동기화용: 파일 이름에 묶인 암호화(이름을 바꾸거나 다른 저장소로 옮기면 복호화가 실패한다).
pub(crate) fn seal_named(key: &VaultKey, name: &str, plain: &[u8]) -> Result<Vec<u8>, VaultError> {
    seal(key, &named_aad(key, name), plain)
}

pub(crate) fn open_named(key: &VaultKey, name: &str, data: &[u8]) -> Option<Vec<u8>> {
    open(key, &named_aad(key, name), data)
}

fn named_aad(key: &VaultKey, name: &str) -> Vec<u8> {
    let mut a = Vec::with_capacity(17 + name.len());
    a.extend_from_slice(&key.vault_id_bytes());
    a.push(b'D');
    a.extend_from_slice(name.as_bytes());
    a
}

// ───────────────────────── manifest ─────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PartEntry {
    /// 이 조각을 올린 세대(안 바뀐 조각은 옛 세대를 가리킨다).
    pub generation: i64,
    pub index: u64,
    /// 평문 길이.
    pub len: u64,
    /// 평문 SHA-256(hex). 안 바뀐 조각을 건너뛰는 근거.
    pub sha256: String,
}

impl PartEntry {
    pub fn name(&self) -> String {
        part_name(self.generation, self.index as usize)
    }
}

/// 서버의 파일 이름: `<세대>.<번호>.part`.
pub fn part_name(generation: i64, index: usize) -> String {
    format!("{generation}.{index:04}.part")
}

fn is_part_name(n: &str) -> bool {
    n.ends_with(".part") && n.split('.').count() == 3
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct VaultManifest {
    pub version: u32,
    /// 이 저장본을 올린 세대.
    pub generation: i64,
    pub device: String,
    pub saved_at: i64,
    pub chunk_size: u64,
    pub total_len: u64,
    /// 전체 평문 SHA-256(hex).
    pub sha256: String,
    pub parts: Vec<PartEntry>,
}

/// 서버에서 읽은 현재 상태. 올릴 때 `base` 로 넘겨 충돌을 감지한다.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteState {
    pub manifest: VaultManifest,
    pub etag: Etag,
}

/// manifest 를 내려받아 복호화한다. 서버에 아무것도 쓰지 않는다.
/// 아직 없으면 `Ok(None)`, 이 키로 풀리지 않으면 `Err(WrongKey)`.
pub fn read_state(remote: &dyn Remote, key: &VaultKey) -> Result<Option<RemoteState>, VaultError> {
    let Some((data, etag)) = remote.get(MANIFEST)? else {
        return Ok(None);
    };
    let plain = open(key, &aad(key, KIND_MANIFEST, 0, 0), &data).ok_or(VaultError::WrongKey)?;
    let manifest: VaultManifest = serde_json::from_slice(&plain).map_err(|e| VaultError::Corrupt(format!("manifest 해석 실패: {e}")))?;
    Ok(Some(RemoteState { manifest, etag }))
}

// ───────────────────────── 올리기 / 받기 ─────────────────────────

#[derive(Debug, Default, Clone, PartialEq)]
pub struct PushStats {
    pub total: usize,
    pub uploaded: usize,
    pub skipped: usize,
    pub removed: usize,
    pub generation: i64,
    /// 새 manifest 의 ETag. 다음 올리기의 `base` 를 만들 때 쓴다.
    pub etag: Etag,
}

fn read_full(f: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        let k = f.read(&mut buf[n..])?;
        if k == 0 {
            break;
        }
        n += k;
    }
    Ok(n)
}

/// DB 스냅숏 파일을 암호화해 올린다.
/// - `base`: 마지막으로 읽은 서버 상태(`read_state` 결과). `None` 이면 "서버에 아직 없다"는 뜻으로 `If-None-Match: *`.
///   서버가 그 사이 바뀌었으면 manifest 올리기에서 `Err(Conflict)`.
/// - 조각은 먼저, manifest 는 마지막에 올리므로 도중에 끊겨도 이전 저장본이 유지된다.
/// - `base` 의 조각과 평문 해시·길이가 같고 서버에 있는 조각은 다시 올리지 않는다.
pub fn push_snapshot(
    remote: &dyn Remote,
    key: &VaultKey,
    snapshot: &Path,
    base: Option<&RemoteState>,
    device: &str,
    now: i64,
    chunk: usize,
) -> Result<PushStats, VaultError> {
    let chunk = chunk.max(1);
    let existing: HashSet<String> = remote.list()?.into_iter().collect();
    // 세대 번호: 서버에 이미 있는 어떤 조각 이름(끊긴 올리기의 찌꺼기 포함)과도 겹치지 않는 값.
    let mut generation = now;
    while existing.iter().any(|n| is_part_name(n) && n.starts_with(&format!("{generation}."))) {
        generation += 1;
    }
    let mut f = File::open(snapshot).map_err(|e| VaultError::Io(format!("스냅숏을 열 수 없음: {e}")))?;
    let mut buf = vec![0u8; chunk];
    let mut all = Sha256::new();
    let mut parts: Vec<PartEntry> = Vec::new();
    let mut st = PushStats { generation, ..Default::default() };
    let mut total = 0u64;
    loop {
        let n = read_full(&mut f, &mut buf).map_err(|e| VaultError::Io(format!("스냅숏 읽기 실패: {e}")))?;
        if n == 0 {
            break;
        }
        let data = &buf[..n];
        all.update(data);
        total += n as u64;
        let i = parts.len();
        let sha = sha_hex(data);
        let same = base
            .and_then(|b| b.manifest.parts.get(i))
            .filter(|p| p.sha256 == sha && p.len == n as u64 && p.index == i as u64 && existing.contains(&p.name()));
        match same {
            Some(p) => {
                parts.push(p.clone());
                st.skipped += 1;
            }
            None => {
                let e = PartEntry { generation, index: i as u64, len: n as u64, sha256: sha };
                let ct = seal(key, &aad(key, KIND_PART, e.generation, e.index), data)?;
                remote.put(&e.name(), &ct, PutCond::Any)?;
                parts.push(e);
                st.uploaded += 1;
            }
        }
        if n < chunk {
            break;
        }
    }
    let m = VaultManifest {
        version: 1,
        generation,
        device: device.to_string(),
        saved_at: now,
        chunk_size: chunk as u64,
        total_len: total,
        sha256: hex(&all.finalize()),
        parts,
    };
    let json = serde_json::to_vec(&m).map_err(|e| VaultError::Corrupt(e.to_string()))?;
    let ct = seal(key, &aad(key, KIND_MANIFEST, 0, 0), &json)?;
    let cond = match base {
        Some(b) => PutCond::IfMatch(b.etag.clone()),
        None => PutCond::IfNoneMatch,
    };
    st.etag = remote.put(MANIFEST, &ct, cond)?;
    st.total = m.parts.len();
    // 이제 manifest 가 가리키지 않는 옛 조각(과 끊긴 올리기의 찌꺼기)을 정리한다. 실패해도 저장본에는 영향 없다.
    let keep: HashSet<String> = m.parts.iter().map(|p| p.name()).collect();
    for n in existing.iter().filter(|n| is_part_name(n) && !keep.contains(*n)) {
        if remote.delete(n).is_ok() {
            st.removed += 1;
        }
    }
    Ok(st)
}

/// manifest 가 가리키는 모든 조각을 받아 복호화·검증하며 `dest` 에 쓴다.
/// 문제가 있으면 `dest` 를 남기지 않고 오류.
pub fn fetch_snapshot(remote: &dyn Remote, key: &VaultKey, m: &VaultManifest, dest: &Path) -> Result<(), VaultError> {
    let r = fetch_inner(remote, key, m, dest);
    if r.is_err() {
        let _ = fs::remove_file(dest);
    }
    r
}

fn fetch_inner(remote: &dyn Remote, key: &VaultKey, m: &VaultManifest, dest: &Path) -> Result<(), VaultError> {
    let mut out = File::create(dest).map_err(|e| VaultError::Io(format!("파일을 만들 수 없음: {e}")))?;
    let mut all = Sha256::new();
    let mut total = 0u64;
    for (i, p) in m.parts.iter().enumerate() {
        if p.index != i as u64 {
            return Err(VaultError::Corrupt(format!("조각 번호가 어긋남: {}", p.name())));
        }
        let (ct, _) = remote.get(&p.name())?.ok_or_else(|| VaultError::Corrupt(format!("조각이 서버에 없음: {}", p.name())))?;
        let plain = open(key, &aad(key, KIND_PART, p.generation, p.index), &ct)
            .ok_or_else(|| VaultError::Corrupt(format!("조각을 복호화할 수 없음(변조 또는 손상): {}", p.name())))?;
        if plain.len() as u64 != p.len || sha_hex(&plain) != p.sha256 {
            return Err(VaultError::Corrupt(format!("조각 내용이 manifest 와 다름: {}", p.name())));
        }
        all.update(&plain);
        total += plain.len() as u64;
        out.write_all(&plain).map_err(|e| VaultError::Io(format!("쓰기 실패: {e}")))?;
    }
    if total != m.total_len || hex(&all.finalize()) != m.sha256 {
        return Err(VaultError::Corrupt("전체 내용이 manifest 와 다름".into()));
    }
    out.sync_all().map_err(|e| VaultError::Io(e.to_string()))?;
    Ok(())
}

// ───────────────────────── 테스트 ─────────────────────────

#[cfg(test)]
pub mod testing {
    //! 테스트용 메모리 `Remote`. 서버처럼 ETag 와 조건부 올리기를 지킨다.
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;

    #[derive(Default)]
    pub struct MemoryRemote {
        files: RefCell<BTreeMap<String, (Vec<u8>, u64)>>,
        counter: Cell<u64>,
        /// Some(n): 앞으로 n번의 put 만 성공하고 그 뒤는 실패(네트워크 끊김 흉내).
        pub fail_after: Cell<Option<usize>>,
        pub puts: Cell<usize>,
    }

    impl MemoryRemote {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn raw(&self, name: &str) -> Option<Vec<u8>> {
            self.files.borrow().get(name).map(|x| x.0.clone())
        }
        pub fn set_raw(&self, name: &str, data: Vec<u8>) {
            let v = self.counter.get() + 1;
            self.counter.set(v);
            self.files.borrow_mut().insert(name.to_string(), (data, v));
        }
        pub fn names(&self) -> Vec<String> {
            self.files.borrow().keys().cloned().collect()
        }
    }

    impl Remote for MemoryRemote {
        fn get(&self, name: &str) -> Result<Option<(Vec<u8>, Etag)>, RemoteError> {
            Ok(self.files.borrow().get(name).map(|(d, v)| (d.clone(), format!("\"{v}\""))))
        }
        fn put(&self, name: &str, data: &[u8], cond: PutCond) -> Result<Etag, RemoteError> {
            self.puts.set(self.puts.get() + 1);
            if let Some(n) = self.fail_after.get() {
                if n == 0 {
                    return Err(RemoteError::Other("연결이 끊김".into()));
                }
                self.fail_after.set(Some(n - 1));
            }
            let cur = self.files.borrow().get(name).map(|x| format!("\"{}\"", x.1));
            match (&cond, &cur) {
                (PutCond::IfNoneMatch, Some(_)) => return Err(RemoteError::Conflict),
                (PutCond::IfMatch(w), c) if c.as_ref() != Some(w) => return Err(RemoteError::Conflict),
                _ => {}
            }
            self.set_raw(name, data.to_vec());
            Ok(format!("\"{}\"", self.counter.get()))
        }
        fn delete(&self, name: &str) -> Result<(), RemoteError> {
            self.files.borrow_mut().remove(name);
            Ok(())
        }
        fn list(&self) -> Result<Vec<String>, RemoteError> {
            Ok(self.names())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::MemoryRemote;
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).unwrap();
        let d = std::env::temp_dir().join(format!("akashic-vault-{tag}-{}", hex(&b)));
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// 재현 가능한 의사 난수 바이트(압축·중복이 없도록).
    fn data(len: usize, seed: u8) -> Vec<u8> {
        let mut x = 0x9e3779b9u32 ^ seed as u32;
        (0..len)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 24) as u8
            })
            .collect()
    }

    fn write(d: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = d.join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    fn key(n: u8) -> VaultKey {
        VaultKey::from_bytes([n; 32])
    }

    #[test]
    fn 복구_문자열_왕복과_형식() {
        let k = VaultKey::generate().unwrap();
        let s = k.recovery_string();
        assert!(s.starts_with("AKS1-"));
        assert_eq!(s.split('-').count(), 15);
        assert!(s.split('-').skip(1).all(|g| g.len() == 4));
        assert_eq!(VaultKey::from_recovery(&s).unwrap(), k);
        // 소문자·공백·하이픈 없음도 허용
        let loose = s.to_ascii_lowercase().replace('-', " ");
        assert_eq!(VaultKey::from_recovery(&loose).unwrap(), k);
    }

    #[test]
    fn 복구_문자열_체크섬_오류() {
        let k = key(7);
        let s = k.recovery_string();
        // 글자 하나를 바꾼다(알파벳 안의 다른 글자로)
        let i = s.len() - 3;
        let c = s.as_bytes()[i] as char;
        let other = if c == 'A' { 'B' } else { 'A' };
        let mut bad = s.clone();
        bad.replace_range(i..i + 1, &other.to_string());
        assert!(VaultKey::from_recovery(&bad).unwrap_err().contains("확인 코드"));
        assert!(VaultKey::from_recovery("AKS1-ABCD").is_err());
        assert!(VaultKey::from_recovery(&s.replace("AKS1", "XXXX")).is_err());
        assert!(VaultKey::from_recovery(&s.replace('-', "").replacen('A', "0", 1)).is_err() || s.find('A').is_none());
    }

    #[test]
    fn 키에서_vault_id가_결정적() {
        let a = key(1);
        assert_eq!(a.vault_id(), key(1).vault_id());
        assert_ne!(a.vault_id(), key(2).vault_id());
        assert_eq!(a.vault_id().len(), 32);
        assert_eq!(a.folder_name(), format!("akashic-{}", a.vault_id()));
        // 복구 문자열을 거쳐도 같다
        assert_eq!(VaultKey::from_recovery(&a.recovery_string()).unwrap().folder_name(), a.folder_name());
        // 마스터 키 자체가 폴더 이름에 드러나지 않는다
        assert!(!a.folder_name().contains(&hex(&[1u8; 32])[..16]));
    }

    #[test]
    fn 올리고_받기_왕복() {
        let d = tmpdir("rt");
        let k = key(3);
        let plain = data(CHUNK * 2 + 1234, 1);
        let snap = write(&d, "snap", &plain);
        let r = MemoryRemote::new();
        let st = push_snapshot(&r, &k, &snap, None, "pc1", 1000, CHUNK).unwrap();
        assert_eq!((st.total, st.uploaded, st.skipped), (3, 3, 0));
        // 서버에는 평문이 없다
        for n in r.names() {
            let raw = r.raw(&n).unwrap();
            assert!(!raw.windows(64).any(|w| plain.windows(64).next() == Some(w)));
        }
        let s = read_state(&r, &k).unwrap().unwrap();
        assert_eq!(s.manifest.device, "pc1");
        assert_eq!(s.manifest.total_len, plain.len() as u64);
        assert_eq!(s.etag, st.etag);
        let out = d.join("out");
        fetch_snapshot(&r, &k, &s.manifest, &out).unwrap();
        assert_eq!(fs::read(&out).unwrap(), plain);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 빈_파일과_폴더_remote_왕복() {
        let d = tmpdir("folder");
        let k = key(4);
        let r = FolderRemote::new(d.join("server").join(k.folder_name()));
        let plain = data(CHUNK + 5, 9);
        let snap = write(&d, "snap", &plain);
        push_snapshot(&r, &k, &snap, None, "pc", 50, CHUNK).unwrap();
        let s = read_state(&r, &k).unwrap().unwrap();
        // 같은 etag 로 다시 올리면 성공, 옛 etag 로 올리면 충돌
        let st2 = push_snapshot(&r, &k, &snap, Some(&s), "pc", 51, CHUNK).unwrap();
        assert_eq!(st2.skipped, 2);
        assert_eq!(push_snapshot(&r, &k, &snap, Some(&s), "pc", 52, CHUNK).unwrap_err(), VaultError::Conflict);
        let s2 = read_state(&r, &k).unwrap().unwrap();
        let out = d.join("out");
        fetch_snapshot(&r, &k, &s2.manifest, &out).unwrap();
        assert_eq!(fs::read(&out).unwrap(), plain);
        // 임시 파일 찌꺼기가 목록에 없다
        assert!(r.list().unwrap().iter().all(|n| !n.ends_with(".akashic-tmp")));
        // 빈 스냅숏
        let empty = write(&d, "empty", b"");
        let s3 = read_state(&r, &k).unwrap().unwrap();
        push_snapshot(&r, &k, &empty, Some(&s3), "pc", 60, CHUNK).unwrap();
        let s4 = read_state(&r, &k).unwrap().unwrap();
        assert!(s4.manifest.parts.is_empty());
        fetch_snapshot(&r, &k, &s4.manifest, &d.join("out2")).unwrap();
        assert_eq!(fs::read(d.join("out2")).unwrap().len(), 0);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 틀린_키는_키가_다릅니다() {
        let d = tmpdir("wk");
        let snap = write(&d, "snap", &data(1000, 2));
        let r = MemoryRemote::new();
        push_snapshot(&r, &key(1), &snap, None, "pc", 1, CHUNK).unwrap();
        let e = read_state(&r, &key(2)).unwrap_err();
        assert_eq!(e, VaultError::WrongKey);
        assert_eq!(e.to_string(), "키가 다릅니다.");
        // 키 확인 단계에서는 서버에 쓰지 않는다
        let before = r.puts.get();
        let _ = read_state(&r, &key(2));
        assert_eq!(r.puts.get(), before);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn manifest_변조_거부() {
        let d = tmpdir("tm");
        let k = key(5);
        let snap = write(&d, "snap", &data(1000, 3));
        let r = MemoryRemote::new();
        push_snapshot(&r, &k, &snap, None, "pc", 1, CHUNK).unwrap();
        let mut m = r.raw(MANIFEST).unwrap();
        let last = m.len() - 1;
        m[last] ^= 1;
        r.set_raw(MANIFEST, m);
        assert!(read_state(&r, &k).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 조각_변조_거부와_결과_파일_정리() {
        let d = tmpdir("tp");
        let k = key(5);
        let snap = write(&d, "snap", &data(CHUNK + 10, 4));
        let r = MemoryRemote::new();
        push_snapshot(&r, &k, &snap, None, "pc", 1, CHUNK).unwrap();
        let s = read_state(&r, &k).unwrap().unwrap();
        let name = s.manifest.parts[1].name();
        let mut p = r.raw(&name).unwrap();
        p[30] ^= 0x55;
        r.set_raw(&name, p);
        let out = d.join("out");
        let e = fetch_snapshot(&r, &k, &s.manifest, &out).unwrap_err();
        assert!(matches!(e, VaultError::Corrupt(_)), "{e:?}");
        assert!(!out.exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 조각_바꿔치기_거부() {
        let d = tmpdir("sw");
        let k = key(6);
        let snap = write(&d, "snap", &data(CHUNK * 2, 5));
        let r = MemoryRemote::new();
        push_snapshot(&r, &k, &snap, None, "pc", 1, CHUNK).unwrap();
        let s = read_state(&r, &k).unwrap().unwrap();
        let (n0, n1) = (s.manifest.parts[0].name(), s.manifest.parts[1].name());
        let (a, b) = (r.raw(&n0).unwrap(), r.raw(&n1).unwrap());
        r.set_raw(&n0, b);
        r.set_raw(&n1, a);
        // 둘 다 올바른 암호문이지만 자리가 다르므로 거부(AAD)
        let e = fetch_snapshot(&r, &k, &s.manifest, &d.join("out")).unwrap_err();
        assert!(matches!(e, VaultError::Corrupt(_)), "{e:?}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 다른_저장소의_조각은_같은_키여도_거부() {
        // 같은 이름·같은 내용 위치라도 vault-id 가 다르면 복호화되지 않는다.
        let d = tmpdir("xv");
        let snap = write(&d, "snap", &data(100, 6));
        let (ra, rb) = (MemoryRemote::new(), MemoryRemote::new());
        push_snapshot(&ra, &key(1), &snap, None, "pc", 1, CHUNK).unwrap();
        push_snapshot(&rb, &key(2), &snap, None, "pc", 1, CHUNK).unwrap();
        let s = read_state(&ra, &key(1)).unwrap().unwrap();
        let name = s.manifest.parts[0].name();
        rb.set_raw(&name, ra.raw(&name).unwrap());
        let sb = read_state(&rb, &key(2)).unwrap().unwrap();
        assert!(fetch_snapshot(&rb, &key(2), &sb.manifest, &d.join("o")).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn nonce는_매번_다르다() {
        let k = key(8);
        let a = seal(&k, b"x", b"hello").unwrap();
        let b = seal(&k, b"x", b"hello").unwrap();
        assert_ne!(a, b);
        assert_eq!(open(&k, b"x", &a).unwrap(), b"hello");
        assert!(open(&k, b"y", &a).is_none());
    }

    #[test]
    fn 동시_올리기는_412_충돌() {
        let d = tmpdir("cf");
        let k = key(9);
        let snap = write(&d, "snap", &data(CHUNK + 1, 7));
        let r = MemoryRemote::new();
        push_snapshot(&r, &k, &snap, None, "pc1", 10, CHUNK).unwrap();
        // 이미 있는데 "없다"고 믿고 올리면 충돌
        assert_eq!(push_snapshot(&r, &k, &snap, None, "pc2", 11, CHUNK).unwrap_err(), VaultError::Conflict);
        // 두 기기가 같은 상태를 읽고, 한쪽이 먼저 올리면 다른 쪽은 충돌
        let base = read_state(&r, &k).unwrap().unwrap();
        let snap2 = write(&d, "snap2", &data(CHUNK + 1, 8));
        push_snapshot(&r, &k, &snap2, Some(&base), "pc1", 12, CHUNK).unwrap();
        let snap3 = write(&d, "snap3", &data(CHUNK + 1, 9));
        assert_eq!(push_snapshot(&r, &k, &snap3, Some(&base), "pc2", 13, CHUNK).unwrap_err(), VaultError::Conflict);
        // 이긴 쪽 저장본이 그대로다
        let s = read_state(&r, &k).unwrap().unwrap();
        assert_eq!(s.manifest.device, "pc1");
        fetch_snapshot(&r, &k, &s.manifest, &d.join("out")).unwrap();
        assert_eq!(fs::read(d.join("out")).unwrap(), fs::read(&snap2).unwrap());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 올리기_도중_끊겨도_이전_저장본_유지_후_복구() {
        let d = tmpdir("ab");
        let k = key(10);
        let v1 = data(CHUNK * 3, 11);
        let snap1 = write(&d, "s1", &v1);
        let r = MemoryRemote::new();
        push_snapshot(&r, &k, &snap1, None, "pc", 100, CHUNK).unwrap();
        let base = read_state(&r, &k).unwrap().unwrap();
        // 전 조각이 바뀐 새 스냅숏을 올리다 두 번째 조각에서 끊김
        let v2 = data(CHUNK * 3, 12);
        let snap2 = write(&d, "s2", &v2);
        r.fail_after.set(Some(1));
        assert!(matches!(push_snapshot(&r, &k, &snap2, Some(&base), "pc", 200, CHUNK).unwrap_err(), VaultError::Remote(_)));
        r.fail_after.set(None);
        // 서버는 여전히 v1 을 온전히 돌려준다
        let s = read_state(&r, &k).unwrap().unwrap();
        assert_eq!(s, base);
        fetch_snapshot(&r, &k, &s.manifest, &d.join("o1")).unwrap();
        assert_eq!(fs::read(d.join("o1")).unwrap(), v1);
        // 다시 올리면 성공하고, 끊긴 올리기의 조각 찌꺼기는 정리된다
        let st = push_snapshot(&r, &k, &snap2, Some(&s), "pc", 200, CHUNK).unwrap();
        assert_ne!(st.generation, 200, "끊긴 올리기가 쓴 세대 번호와 겹치면 안 된다");
        let s2 = read_state(&r, &k).unwrap().unwrap();
        fetch_snapshot(&r, &k, &s2.manifest, &d.join("o2")).unwrap();
        assert_eq!(fs::read(d.join("o2")).unwrap(), v2);
        let parts: Vec<_> = r.names().into_iter().filter(|n| n.ends_with(".part")).collect();
        assert_eq!(parts.len(), 3, "{parts:?}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 안_바뀐_조각은_건너뛴다() {
        let d = tmpdir("sk");
        let k = key(11);
        let mut v = data(CHUNK * 3, 13);
        let snap = write(&d, "s", &v);
        let r = MemoryRemote::new();
        push_snapshot(&r, &k, &snap, None, "pc", 1, CHUNK).unwrap();
        let base = read_state(&r, &k).unwrap().unwrap();
        // 가운데 조각만 바꾼다
        v[CHUNK + 5] ^= 0xff;
        let snap2 = write(&d, "s2", &v);
        let puts_before = r.puts.get();
        let st = push_snapshot(&r, &k, &snap2, Some(&base), "pc", 2, CHUNK).unwrap();
        assert_eq!((st.uploaded, st.skipped), (1, 2));
        assert_eq!(r.puts.get() - puts_before, 2, "바뀐 조각 1개 + manifest");
        assert_eq!(st.removed, 1, "바뀐 조각의 옛 판본만 정리");
        let s = read_state(&r, &k).unwrap().unwrap();
        // 건너뛴 조각은 옛 세대를 계속 가리킨다
        assert_eq!(s.manifest.parts[0].generation, 1);
        assert_eq!(s.manifest.parts[1].generation, 2);
        fetch_snapshot(&r, &k, &s.manifest, &d.join("o")).unwrap();
        assert_eq!(fs::read(d.join("o")).unwrap(), v);
        // 서버에서 조각이 사라졌다면 건너뛰지 않고 다시 올린다
        r.delete(&s.manifest.parts[0].name()).unwrap();
        let st = push_snapshot(&r, &k, &snap2, Some(&s), "pc", 3, CHUNK).unwrap();
        assert_eq!((st.uploaded, st.skipped), (1, 2));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 아직_없는_저장소는_none() {
        let r = MemoryRemote::new();
        assert!(read_state(&r, &key(1)).unwrap().is_none());
        let f = FolderRemote::new(std::env::temp_dir().join("akashic-vault-없는-폴더-xyz"));
        assert!(read_state(&f, &key(1)).unwrap().is_none());
        assert!(f.list().unwrap().is_empty());
    }

    #[test]
    fn 폴더_remote는_경로_탈출_이름을_거부() {
        let f = FolderRemote::new(std::env::temp_dir());
        assert!(f.get("../x").is_err());
        assert!(f.put("a/b", b"1", PutCond::Any).is_err());
        assert!(f.delete("..").is_err());
    }
}
