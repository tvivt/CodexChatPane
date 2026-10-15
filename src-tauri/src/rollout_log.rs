use std::{
    collections::HashMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub path: PathBuf,
    pub len: u64,
    pub modified: Option<SystemTime>,
}

pub(crate) struct Log {
    file: File,
    pub stamp: Stamp,
    pub compressed: bool,
}

pub(crate) fn logical_path(path: &Path) -> PathBuf {
    if path.extension().is_some_and(|extension| extension == "zst") {
        path.with_extension("")
    } else {
        path.to_path_buf()
    }
}

pub(crate) fn is_log(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "jsonl")
        || path
            .to_str()
            .is_some_and(|name| name.ends_with(".jsonl.zst"))
}

pub(crate) fn fragment_id(path: &Path) -> Option<&str> {
    path.file_name()?
        .to_str()?
        .strip_suffix(".zst")
        .unwrap_or(path.file_name()?.to_str()?)
        .strip_suffix(".jsonl")?
        .rsplit_once('_')
        .map(|(_, id)| id)
}

pub(crate) fn open(path: &Path) -> io::Result<Log> {
    let plain = logical_path(path);
    let compressed = plain.with_extension("jsonl.zst");
    // Resolve again if compression renamed the files between the two opens.
    for _ in 0..2 {
        for (candidate, compressed) in [(&plain, false), (&compressed, true)] {
            match File::open(candidate) {
                Ok(file) => {
                    let metadata = file.metadata()?;
                    return Ok(Log {
                        file,
                        stamp: Stamp {
                            path: candidate.clone(),
                            len: metadata.len(),
                            modified: metadata.modified().ok(),
                        },
                        compressed,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("无法定位对话日志：{}", plain.display()),
    ))
}

impl Log {
    pub fn len(&mut self) -> io::Result<u64> {
        if !self.compressed {
            return Ok(self.stamp.len);
        }
        static LENGTHS: OnceLock<Mutex<HashMap<PathBuf, (Stamp, u64)>>> = OnceLock::new();
        let cache = LENGTHS.get_or_init(Default::default);
        if let Some((_, len)) = cache
            .lock()
            .map_err(|error| io::Error::other(error.to_string()))?
            .get(&self.stamp.path)
            .filter(|(stamp, _)| stamp == &self.stamp)
        {
            return Ok(*len);
        }
        self.file.rewind()?;
        let len = io::copy(
            &mut zstd::stream::read::Decoder::new(&mut self.file)?,
            &mut io::sink(),
        )?;
        self.file.rewind()?;
        let mut cache = cache
            .lock()
            .map_err(|error| io::Error::other(error.to_string()))?;
        if cache.len() >= 128 {
            cache.clear();
        }
        cache.insert(self.stamp.path.clone(), (self.stamp.clone(), len));
        Ok(len)
    }

    pub fn reader_at(mut self, offset: u64) -> io::Result<Box<dyn Read + Send>> {
        if !self.compressed {
            self.file.seek(SeekFrom::Start(offset))?;
            return Ok(Box::new(self.file));
        }
        self.file.rewind()?;
        let mut reader = zstd::stream::read::Decoder::new(self.file)?;
        if io::copy(&mut reader.by_ref().take(offset), &mut io::sink())? != offset {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "对话日志偏移超出解压后的长度",
            ));
        }
        Ok(Box::new(reader))
    }
}

#[cfg(test)]
pub(crate) fn compress_fixture(path: &Path) {
    let encoded = zstd::stream::encode_all(File::open(path).unwrap(), 1).unwrap();
    std::fs::write(path.with_extension("jsonl.zst"), encoded).unwrap();
    std::fs::rename(path, path.with_extension("fixture-original")).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_compression_after_open_and_keeps_logical_offsets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-main_fragment.jsonl");
        let text = "first\n汉字\nlast\n";
        std::fs::write(&path, text).unwrap();
        let original = open(&path).unwrap();
        let plain_stamp = original.stamp.clone();
        compress_fixture(&path);
        let mut log = open(&path).unwrap();
        assert_ne!(plain_stamp, log.stamp);
        assert!(log.compressed);
        assert_eq!(log.len().unwrap(), text.len() as u64);
        let mut tail = String::new();
        log.reader_at(6).unwrap().read_to_string(&mut tail).unwrap();
        assert_eq!(tail, "汉字\nlast\n");
        let mut retained = String::new();
        original
            .reader_at(0)
            .unwrap()
            .read_to_string(&mut retained)
            .unwrap();
        assert_eq!(retained, text);
        assert_eq!(
            fragment_id(&path.with_extension("jsonl.zst")),
            Some("fragment")
        );
    }

    #[test]
    fn compressed_length_and_reader_include_all_frames_and_reject_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl.zst");
        let mut bytes = zstd::stream::encode_all(&b"one\n"[..], 1).unwrap();
        bytes.extend(zstd::stream::encode_all(&b"two\n"[..], 1).unwrap());
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(open(&path).unwrap().len().unwrap(), 8);
        assert!(open(&path).unwrap().reader_at(9).is_err());
        std::fs::write(&path, b"broken").unwrap();
        assert!(open(&path).unwrap().len().is_err());
    }
}
