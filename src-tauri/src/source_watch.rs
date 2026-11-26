#[cfg(windows)]
mod windows {
    use std::{
        collections::HashSet, ffi::c_void, os::windows::ffi::OsStrExt, path::Path, sync::Arc,
        thread, time::Duration,
    };
    use tauri::Emitter;

    type Handle = *mut c_void;
    const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    const FILE_LIST_DIRECTORY: u32 = 0x0001;
    const FILE_SHARE_READ: u32 = 0x0001;
    const FILE_SHARE_WRITE: u32 = 0x0002;
    const FILE_SHARE_DELETE: u32 = 0x0004;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const CHANGE_FILTER: u32 = 0x0001 | 0x0002 | 0x0008 | 0x0010 | 0x0040;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            creation: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
        fn ReadDirectoryChangesW(
            directory: Handle,
            buffer: *mut c_void,
            length: u32,
            subtree: i32,
            filter: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
            completion: *mut c_void,
        ) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    #[derive(Clone, Default, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Change {
        pub full: bool,
        pub catalog: bool,
        pub history: bool,
        pub threads: Vec<String>,
        #[serde(skip)]
        settings: bool,
    }

    fn thread_id(path: &str) -> Option<String> {
        let name = path.rsplit('\\').next()?;
        let id = name.strip_prefix("rollout-")?.get(20..56)?;
        ((name.ends_with(".jsonl") || name.ends_with(".jsonl.zst"))
            && id.len() == 36
            && id.bytes().enumerate().all(|(index, byte)| {
                if [8, 13, 18, 23].contains(&index) {
                    byte == b'-'
                } else {
                    byte.is_ascii_hexdigit()
                }
            }))
        .then(|| id.to_ascii_lowercase())
    }

    fn classify(path: &str, change: &mut Change, ids: &mut HashSet<String>) {
        let lower = path.to_ascii_lowercase();
        let name = lower.rsplit('\\').next().unwrap_or(&lower);
        if name == ".codex-global-state.sqlite" || name == ".codex-global-state.sqlite-wal" {
            change.settings = true;
        } else if name == ".codex-global-state.json"
            || name == "session_index.jsonl"
            || name.starts_with("state_5.sqlite")
        {
            change.catalog = true;
        } else if name == "thread_history_1.sqlite" || name == "thread_history_1.sqlite-wal" {
            change.history = true;
        } else if lower == "sessions" || lower == "archived_sessions" {
            change.catalog = true;
        } else if (lower.starts_with("sessions\\") || lower.starts_with("archived_sessions\\"))
            && thread_id(&lower).is_some()
        {
            ids.insert(thread_id(&lower).unwrap());
        }
    }

    fn open_directory(name: &[u16]) -> Handle {
        unsafe {
            CreateFileW(
                name.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            )
        }
    }

    pub fn start(app: tauri::AppHandle, root: &Path) -> Result<(), String> {
        watch(
            Arc::new(move |change| {
                let _ = app.emit("codex-source-changed", change);
            }),
            root,
            String::new(),
            false,
        )
    }

    fn watch(
        emit: Arc<dyn Fn(Change) + Send + Sync>,
        root: &Path,
        prefix: String,
        recursive: bool,
    ) -> Result<(), String> {
        let name: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
        let directory = open_directory(&name);
        if directory == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let directory = directory as usize;
        let root = root.to_path_buf();
        let mut settings = crate::source::global_state_settings(&root).ok();
        thread::spawn(move || {
            let mut directory = directory as Handle;
            let mut buffer = [0u8; 64 * 1024];
            let mut watched = HashSet::new();
            loop {
                loop {
                    if !recursive {
                        for folder in ["sessions", "archived_sessions"] {
                            if !watched.contains(folder)
                                && root.join(folder).is_dir()
                                && watch(
                                    emit.clone(),
                                    &root.join(folder),
                                    format!("{folder}\\"),
                                    true,
                                )
                                .is_ok()
                            {
                                watched.insert(folder);
                            }
                        }
                    }
                    let mut returned = 0u32;
                    let ok = unsafe {
                        ReadDirectoryChangesW(
                            directory,
                            buffer.as_mut_ptr().cast(),
                            buffer.len() as u32,
                            i32::from(recursive),
                            CHANGE_FILTER,
                            &mut returned,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                        )
                    };
                    if ok == 0 {
                        break;
                    }
                    let mut change = Change::default();
                    let mut ids = HashSet::new();
                    if returned == 0 {
                        change.full = true; // A full buffer can lose individual file names.
                    }
                    let mut offset = 0usize;
                    while offset + 12 <= returned as usize {
                        let next =
                            u32::from_ne_bytes(buffer[offset..offset + 4].try_into().unwrap())
                                as usize;
                        let length =
                            u32::from_ne_bytes(buffer[offset + 8..offset + 12].try_into().unwrap())
                                as usize;
                        if offset + 12 + length > returned as usize || length % 2 != 0 {
                            change.full = true;
                            break;
                        }
                        let path = String::from_utf16_lossy(
                            &buffer[offset + 12..offset + 12 + length]
                                .chunks_exact(2)
                                .map(|bytes| u16::from_ne_bytes([bytes[0], bytes[1]]))
                                .collect::<Vec<_>>(),
                        );
                        let action =
                            u32::from_ne_bytes(buffer[offset + 4..offset + 8].try_into().unwrap());
                        classify(&format!("{prefix}{path}"), &mut change, &mut ids);
                        // A rollout can move or be replaced under the same thread ID.
                        if recursive && action != 3 {
                            change.catalog = true;
                        }
                        if next == 0 {
                            break;
                        }
                        offset += next;
                    }
                    change.threads = ids.into_iter().collect();
                    if change.settings {
                        // A WAL notification can arrive before its commit becomes visible to readers.
                        for delay in [0, 25, 75] {
                            if delay > 0 {
                                thread::sleep(Duration::from_millis(delay));
                            }
                            match crate::source::global_state_settings(&root) {
                                Ok(current) if settings.as_ref() != Some(&current) => {
                                    change.catalog = true;
                                    settings = Some(current);
                                    break;
                                }
                                Err(_) if delay == 75 => change.catalog = true,
                                _ => {}
                            }
                        }
                    }
                    if change.full || change.catalog || change.history || !change.threads.is_empty()
                    {
                        emit(change);
                    }
                }
                unsafe { CloseHandle(directory) };
                emit(Change {
                    full: true,
                    ..Default::default()
                });
                loop {
                    thread::sleep(Duration::from_secs(2));
                    directory = open_directory(&name);
                    if directory != INVALID_HANDLE_VALUE {
                        break;
                    }
                }
                emit(Change {
                    full: true,
                    ..Default::default()
                });
            }
        });
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn metadata_notifications() {
            use rusqlite::Connection;
            use std::{fs, sync::mpsc, time::Instant};
            let dir = tempfile::tempdir().unwrap();
            let database = dir.path().join(".codex-global-state.sqlite");
            let db = Connection::open(&database).unwrap();
            db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE settings(scope TEXT,key TEXT,value TEXT,PRIMARY KEY(scope,key)); INSERT INTO settings VALUES('global','thread-project-assignments','{}');").unwrap();
            let (sender, receiver) = mpsc::channel();
            watch(
                Arc::new(move |c| {
                    let _ = sender.send(c);
                }),
                dir.path(),
                String::new(),
                false,
            )
            .unwrap();
            // Ensure the blocking OS read is armed before mutating the isolated files.
            thread::sleep(Duration::from_millis(50));
            db.execute(
                "INSERT INTO settings VALUES('global','window-state','{}')",
                [],
            )
            .unwrap();
            assert!(
                receiver.recv_timeout(Duration::from_millis(250)).is_err(),
                "unrelated settings must not emit a catalog refresh"
            );
            db.execute(
                "INSERT INTO settings VALUES('other','thread-project-assignments','{}')",
                [],
            )
            .unwrap();
            assert!(
                receiver.recv_timeout(Duration::from_millis(250)).is_err(),
                "non-global settings must not refresh"
            );
            let started = Instant::now();
            db.execute("UPDATE settings SET value=?1 WHERE scope='global' AND key='thread-project-assignments'",[r#"{"thread":{"projectId":"p"}}"#]).unwrap();
            assert!(
                receiver
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .catalog
            );
            eprintln!(
                "metadata global WAL -> catalog event: {:?}",
                started.elapsed()
            );
            let mut longest = Duration::ZERO;
            for _ in 0..10 {
                for (key, value) in [
                    (
                        "electron-thread-read-state-v1",
                        r#"{"unreadByIdentity":{"x":{"local":["a"]}}}"#,
                    ),
                    (
                        "electron-thread-read-state-v1",
                        r#"{"unreadByIdentity":{}}"#,
                    ),
                    ("pinned-thread-ids", r#"["a"]"#),
                    ("pinned-thread-ids", "[]"),
                    ("pinned-project-ids", r#"["p"]"#),
                    ("pinned-project-ids", "[]"),
                    ("project-order", r#"["p","q"]"#),
                    ("project-order", r#"["q","p"]"#),
                ] {
                    while receiver.recv_timeout(Duration::from_millis(30)).is_ok() {}
                    let started = Instant::now();
                    db.execute("INSERT INTO settings VALUES('global',?1,?2) ON CONFLICT(scope,key) DO UPDATE SET value=excluded.value",[key,value]).unwrap();
                    assert!(
                        receiver
                            .recv_timeout(Duration::from_secs(2))
                            .unwrap_or_else(|error| panic!("{key}={value}: {error}"))
                            .catalog,
                        "{key} must notify the pane"
                    );
                    longest = longest.max(started.elapsed());
                }
            }
            eprintln!("metadata 80 read/pin/order transitions -> catalog event: max {longest:?}");
            // Wait for any duplicate write events to drain before the next measured trigger.
            while receiver.recv_timeout(Duration::from_millis(100)).is_ok() {}
            let started = Instant::now();
            fs::write(dir.path().join("session_index.jsonl"), "{}\n").unwrap();
            assert!(
                receiver
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .catalog
            );
            eprintln!(
                "metadata title index -> catalog event: {:?}",
                started.elapsed()
            );
        }

        #[test]
        fn classifies_multiple_conversations() {
            let mut change = Change::default();
            let mut ids = HashSet::new();
            for id in [
                "01a0cd2a-48fe-7991-8579-b85b5d06ce51",
                "01a0c86d-7ae6-7ea1-92b5-382f9e6bbc0b",
            ] {
                for suffix in ["jsonl", "jsonl.zst"] {
                    classify(
                        &format!(
                            "sessions\\2026\\09\\23\\rollout-2026-09-23T15-38-49-{id}.{suffix}"
                        ),
                        &mut change,
                        &mut ids,
                    );
                    classify(
                        &format!("archived_sessions\\rollout-2026-09-23T15-38-49-{id}.{suffix}"),
                        &mut change,
                        &mut ids,
                    );
                }
            }
            assert_eq!(ids.len(), 2);
            assert!(!change.full);
            classify("state_5.sqlite-wal", &mut change, &mut ids);
            classify("thread_history_1.sqlite-wal", &mut change, &mut ids);
            assert!(change.catalog && change.history);
            let mut ignored = Change::default();
            classify("thread_history_1.sqlite-shm", &mut ignored, &mut ids);
            classify("logs\\codex.log", &mut ignored, &mut ids);
            assert!(!ignored.full && !ignored.catalog && !ignored.history);
        }
    }
}

#[cfg(windows)]
pub use windows::start;

#[cfg(not(windows))]
pub fn start(_: tauri::AppHandle, _: &std::path::Path) -> Result<(), String> {
    Err("source watcher is only available on Windows".into())
}
