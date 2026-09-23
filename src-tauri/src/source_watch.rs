#[cfg(windows)]
mod windows {
    use std::{
        collections::HashSet, ffi::c_void, os::windows::ffi::OsStrExt, path::Path, thread,
        time::Duration,
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
    }

    fn thread_id(path: &str) -> Option<String> {
        let name = path.rsplit('\\').next()?;
        let id = name.strip_prefix("rollout-")?.get(20..56)?;
        (name.ends_with(".jsonl")
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
        if name == ".codex-global-state.json" || name.starts_with("state_5.sqlite") {
            change.catalog = true;
        } else if name.starts_with("thread_history_1.sqlite") {
            change.history = true;
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
        let name: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
        let directory = open_directory(&name);
        if directory == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let directory = directory as usize;
        thread::spawn(move || {
            let mut directory = directory as Handle;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                loop {
                    let mut returned = 0u32;
                    let ok = unsafe {
                        ReadDirectoryChangesW(
                            directory,
                            buffer.as_mut_ptr().cast(),
                            buffer.len() as u32,
                            1,
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
                        classify(&path, &mut change, &mut ids);
                        if next == 0 {
                            break;
                        }
                        offset += next;
                    }
                    change.threads = ids.into_iter().collect();
                    if change.full || change.catalog || change.history || !change.threads.is_empty()
                    {
                        let _ = app.emit("codex-source-changed", change);
                    }
                }
                unsafe { CloseHandle(directory) };
                let _ = app.emit(
                    "codex-source-changed",
                    Change {
                        full: true,
                        ..Default::default()
                    },
                );
                loop {
                    thread::sleep(Duration::from_secs(2));
                    directory = open_directory(&name);
                    if directory != INVALID_HANDLE_VALUE {
                        break;
                    }
                }
                let _ = app.emit(
                    "codex-source-changed",
                    Change {
                        full: true,
                        ..Default::default()
                    },
                );
            }
        });
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn classifies_multiple_conversations() {
            let mut change = Change::default();
            let mut ids = HashSet::new();
            for id in [
                "01a0cd2a-48fe-7991-8579-b85b5d06ce51",
                "01a0c86d-7ae6-7ea1-92b5-382f9e6bbc0b",
            ] {
                classify(
                    &format!("sessions\\2026\\09\\23\\rollout-2026-09-23T15-38-49-{id}.jsonl"),
                    &mut change,
                    &mut ids,
                );
            }
            assert_eq!(ids.len(), 2);
            assert!(!change.full);
            classify("state_5.sqlite-wal", &mut change, &mut ids);
            classify("thread_history_1.sqlite-wal", &mut change, &mut ids);
            assert!(change.catalog && change.history);
        }
    }
}

#[cfg(windows)]
pub use windows::start;

#[cfg(not(windows))]
pub fn start(_: tauri::AppHandle, _: &std::path::Path) -> Result<(), String> {
    Err("source watcher is only available on Windows".into())
}
