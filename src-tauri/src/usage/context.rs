use rusqlite::{params, Connection, OptionalExtension};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone)]
struct Account {
    id: String,
    auth: Option<(u64, std::time::SystemTime)>,
}

fn auth_stamp(home: &Path) -> Result<Option<(u64, std::time::SystemTime)>, String> {
    match std::fs::metadata(home.join("auth.json")) {
        Ok(m) => Ok(Some((m.len(), m.modified().map_err(|e| e.to_string())?))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn accounts() -> &'static Mutex<HashMap<PathBuf, Account>> {
    static ACCOUNTS: OnceLock<Mutex<HashMap<PathBuf, Account>>> = OnceLock::new();
    ACCOUNTS.get_or_init(Default::default)
}

// Share only active work; later calls must check the current file signatures again.
pub(crate) fn share(key: String, work: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    type Task = Arc<OnceLock<Result<(), String>>>;
    static TASKS: OnceLock<Mutex<HashMap<String, Task>>> = OnceLock::new();
    let tasks = TASKS.get_or_init(Default::default);
    let task = tasks
        .lock()
        .map_err(|_| "读取任务锁已损坏")?
        .entry(key.clone())
        .or_default()
        .clone();
    let result = task.get_or_init(work).clone();
    let mut tasks = tasks.lock().map_err(|_| "读取任务锁已损坏")?;
    if tasks
        .get(&key)
        .is_some_and(|current| Arc::ptr_eq(current, &task))
    {
        tasks.remove(&key);
    }
    result
}

pub fn confirm(home: &Path, account: Option<&str>) -> Result<(), String> {
    let mut accounts = accounts().lock().map_err(|_| "账号状态锁已损坏")?;
    if let Some(account) = account.filter(|s| !s.is_empty()) {
        accounts.insert(
            home.to_path_buf(),
            Account {
                id: account.into(),
                auth: auth_stamp(home)?,
            },
        );
    } else {
        accounts.remove(home);
    }
    Ok(())
}

#[cfg(windows)]
fn machine() -> Result<String, String> {
    use std::ffi::c_void;
    #[link(name = "advapi32")]
    extern "system" {
        fn RegGetValueW(
            key: *mut c_void,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            size: *mut u32,
        ) -> i32;
    }
    let subkey: Vec<u16> = "SOFTWARE\\Microsoft\\Cryptography\0"
        .encode_utf16()
        .collect();
    let value: Vec<u16> = "MachineGuid\0".encode_utf16().collect();
    let mut data = [0u16; 128];
    let mut size = (data.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            0x80000002u32 as i32 as isize as *mut c_void,
            subkey.as_ptr(),
            value.as_ptr(),
            0x10002,
            std::ptr::null_mut(),
            data.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return Err(format!("无法读取机器标识：{status}"));
    }
    let end = data.iter().position(|v| *v == 0).unwrap_or(data.len());
    if end == 0 {
        return Err("机器标识为空".into());
    }
    String::from_utf16(&data[..end]).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn machine() -> Result<String, String> {
    std::fs::read_to_string("/etc/machine-id").map_err(|e| e.to_string())
}

pub fn identity(home: &Path) -> Result<String, String> {
    static MACHINE: OnceLock<Result<String, String>> = OnceLock::new();
    let account = accounts()
        .lock()
        .map_err(|_| "账号状态锁已损坏")?
        .get(home)
        .cloned()
        .ok_or("尚未确认当前账号，消耗缓存暂不可用")?;
    if account.auth != auth_stamp(home)? {
        return Err("登录资料已变化，等待重新确认当前账号".into());
    }
    let path = home.canonicalize().map_err(|e| e.to_string())?;
    let catalog = std::fs::metadata(home.join("state_5.sqlite"))
        .ok()
        .and_then(|m| m.created().ok());
    // A copied cache must not survive a machine, account or source-catalog replacement.
    Ok(format!(
        "v2|{}|{}|{catalog:?}|{}",
        MACHINE
            .get_or_init(machine)
            .as_ref()
            .map_err(Clone::clone)?,
        path.display(),
        account.id
    ))
}

pub(super) fn account(home: &Path) -> Result<String, String> {
    identity(home)?;
    accounts()
        .lock()
        .map_err(|_| "账号状态锁已损坏")?
        .get(home)
        .map(|a| a.id.clone())
        .ok_or_else(|| "尚未确认当前账号".into())
}

pub(crate) fn database(home: &Path) -> Result<Connection, String> {
    let identity = identity(home)?;
    let hash = identity.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    let directory = home.join(".codex-chat-pane");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let db = super::open_database(&directory.join(format!("usage-cache-{hash:016x}.sqlite")))?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS cache_identity(value TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS usage_reports(key TEXT PRIMARY KEY,stamp TEXT NOT NULL,data TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS usage_indexed_ranges(key TEXT PRIMARY KEY,from_at INTEGER,to_at INTEGER);
        CREATE TABLE IF NOT EXISTS usage_days(day INTEGER PRIMARY KEY,revision INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS usage_metadata(id INTEGER PRIMARY KEY,revision INTEGER NOT NULL);
        INSERT OR IGNORE INTO usage_metadata VALUES(1,0);
        CREATE TRIGGER IF NOT EXISTS usage_insert AFTER INSERT ON usage_requests BEGIN
          INSERT INTO usage_days VALUES(COALESCE(NEW.at/86400000,-1),1) ON CONFLICT(day) DO UPDATE SET revision=revision+1; END;
        CREATE TRIGGER IF NOT EXISTS usage_delete AFTER DELETE ON usage_requests BEGIN
          INSERT INTO usage_days VALUES(COALESCE(OLD.at/86400000,-1),1) ON CONFLICT(day) DO UPDATE SET revision=revision+1; END;
        CREATE TRIGGER IF NOT EXISTS usage_update AFTER UPDATE ON usage_requests BEGIN
          INSERT INTO usage_days VALUES(COALESCE(OLD.at/86400000,-1),1) ON CONFLICT(day) DO UPDATE SET revision=revision+1;
          INSERT INTO usage_days VALUES(COALESCE(NEW.at/86400000,-1),1) ON CONFLICT(day) DO UPDATE SET revision=revision+1; END;
        CREATE TRIGGER IF NOT EXISTS metadata_insert AFTER INSERT ON usage_threads BEGIN
          UPDATE usage_metadata SET revision=revision+1; END;
        CREATE TRIGGER IF NOT EXISTS metadata_update AFTER UPDATE ON usage_threads
          WHEN NEW.title IS NOT OLD.title OR NEW.project_id IS NOT OLD.project_id OR NEW.project IS NOT OLD.project
            OR NEW.parent IS NOT OLD.parent OR NEW.error IS NOT OLD.error BEGIN
          UPDATE usage_metadata SET revision=revision+1; END;") .map_err(|e| e.to_string())?;
    let saved: Option<String> = db
        .query_row("SELECT value FROM cache_identity LIMIT 1", [], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    if saved.as_ref().is_some_and(|s| s != &identity) {
        return Err("消耗缓存身份不匹配".into());
    }
    if saved.is_none() {
        db.execute("INSERT INTO cache_identity VALUES(?1)", params![identity])
            .map_err(|e| e.to_string())?;
    }
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_reads_share_success_and_error_but_later_reads_retry() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Barrier,
        };
        for result in [Ok(()), Err("unreadable".to_string())] {
            let count = AtomicUsize::new(0);
            let ready = Barrier::new(5);
            std::thread::scope(|scope| {
                let handles: Vec<_> = (0..4)
                    .map(|_| {
                        scope.spawn(|| {
                            ready.wait();
                            share("test-concurrent".into(), || {
                                count.fetch_add(1, Ordering::SeqCst);
                                std::thread::sleep(std::time::Duration::from_millis(100));
                                result.clone()
                            })
                        })
                    })
                    .collect();
                ready.wait();
                for handle in handles {
                    assert_eq!(handle.join().unwrap(), result);
                }
            });
            assert_eq!(count.load(Ordering::SeqCst), 1);
            share("test-concurrent".into(), || {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .unwrap();
            assert_eq!(count.load(Ordering::SeqCst), 2);
        }
    }
    #[test]
    fn copied_cache_with_another_machine_identity_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        confirm(dir.path(), Some("account")).unwrap();
        let db = database(dir.path()).unwrap();
        let original = identity(dir.path()).unwrap();
        let parts: Vec<_> = original.split('|').collect();
        let other = original.replacen(parts[1], "another-machine", 1);
        db.execute("UPDATE cache_identity SET value=?1", [other])
            .unwrap();
        drop(db);
        assert!(database(dir.path()).is_err());
    }
    #[test]
    fn account_and_source_changes_isolate_persistent_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(database(dir.path()).is_err());
        confirm(dir.path(), Some("a")).unwrap();
        database(dir.path())
            .unwrap()
            .execute(
                "INSERT INTO usage_threads VALUES('a','','A','','',NULL,NULL)",
                [],
            )
            .unwrap();
        let first = identity(dir.path()).unwrap();
        confirm(dir.path(), Some("b")).unwrap();
        assert_eq!(
            database(dir.path())
                .unwrap()
                .query_row("SELECT count(*) FROM usage_threads", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        std::fs::write(dir.path().join("auth.json"), "changed credentials").unwrap();
        assert!(
            identity(dir.path()).is_err(),
            "login changes pause reuse before the next quota query"
        );
        confirm(dir.path(), Some("a")).unwrap();
        assert!(identity(dir.path()).is_ok());
        confirm(dir.path(), None).unwrap();
        assert!(database(dir.path()).is_err());
        confirm(dir.path(), Some("a")).unwrap();
        assert_eq!(identity(dir.path()).unwrap(), first);
        assert_eq!(
            database(dir.path())
                .unwrap()
                .query_row("SELECT count(*) FROM usage_threads", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        std::fs::write(dir.path().join("state_5.sqlite"), "new source").unwrap();
        assert_ne!(identity(dir.path()).unwrap(), first);
        assert_eq!(
            database(dir.path())
                .unwrap()
                .query_row("SELECT count(*) FROM usage_threads", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
