use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

#[cfg(test)]
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct ReadStats {
    pub opens: u64,
    pub decoded: u64,
    pub bytes: u64,
}
#[cfg(test)]
thread_local! { static STATS:std::cell::Cell<ReadStats> = std::cell::Cell::new(ReadStats::default()); }
#[cfg(test)]
pub(crate) fn read_stats(reset: bool) -> ReadStats {
    STATS.with(|stats| {
        let value = stats.get();
        if reset {
            stats.set(ReadStats::default());
        }
        value
    })
}
#[cfg(test)]
struct CountReader(Box<dyn Read + Send>);
#[cfg(test)]
impl Read for CountReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let n = self.0.read(bytes)?;
        STATS.with(|s| {
            let mut v = s.get();
            v.bytes += n as u64;
            s.set(v)
        });
        Ok(n)
    }
}
fn tracked(reader: Box<dyn Read + Send>) -> Box<dyn Read + Send> {
    #[cfg(test)]
    {
        return Box::new(CountReader(reader));
    }
    #[cfg(not(test))]
    {
        reader
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Stamp {
    pub path: PathBuf,
    pub len: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
}

pub(crate) struct Log {
    file: File,
    pub stamp: Stamp,
    pub compressed: bool,
}

fn lengths() -> &'static Mutex<Vec<(Stamp, u64)>> {
    static LENGTHS: OnceLock<Mutex<Vec<(Stamp, u64)>>> = OnceLock::new();
    LENGTHS.get_or_init(Default::default)
}
pub(crate) fn remember_length(stamp: &Stamp, len: u64) {
    if let Ok(mut cache) = lengths().lock() {
        cache.retain(|(old, _)| old.path != stamp.path);
        cache.push((stamp.clone(), len));
        if cache.len() > 128 {
            cache.remove(0);
        }
    }
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
    #[cfg(test)]
    STATS.with(|s| {
        let mut v = s.get();
        v.opens += 1;
        s.set(v)
    });
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
                            created: metadata.created().ok(),
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
        {
            let mut cache = lengths()
                .lock()
                .map_err(|e| io::Error::other(e.to_string()))?;
            if let Some(index) = cache.iter().position(|(stamp, _)| stamp == &self.stamp) {
                let value = cache.remove(index);
                let len = value.1;
                cache.push(value);
                return Ok(len);
            }
        }
        self.file.rewind()?;
        #[cfg(test)]
        STATS.with(|s| {
            let mut v = s.get();
            v.decoded += 1;
            s.set(v)
        });
        let len = io::copy(
            &mut zstd::stream::read::Decoder::new(&mut self.file)?,
            &mut io::sink(),
        )?;
        #[cfg(test)]
        STATS.with(|s| {
            let mut v = s.get();
            v.bytes += len;
            s.set(v)
        });
        self.file.rewind()?;
        remember_length(&self.stamp, len);
        Ok(len)
    }

    pub fn reader_at(mut self, offset: u64) -> io::Result<Box<dyn Read + Send>> {
        if !self.compressed {
            self.file.seek(SeekFrom::Start(offset))?;
            return Ok(tracked(Box::new(self.file)));
        }
        self.file.rewind()?;
        #[cfg(test)]
        STATS.with(|s| {
            let mut v = s.get();
            v.decoded += 1;
            s.set(v)
        });
        let mut reader = tracked(Box::new(zstd::stream::read::Decoder::new(self.file)?));
        if io::copy(&mut reader.by_ref().take(offset), &mut io::sink())? != offset {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "对话日志偏移超出解压后的长度",
            ));
        }
        Ok(reader)
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
