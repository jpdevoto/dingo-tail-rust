// Copyright (C) 2026 Juan Pablo Devoto
// SPDX-License-Identifier: GPL-3.0-only

use crate::{
    model::*,
    network::{View, consume},
};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Encoding {
    Utf8,
    Le,
    Be,
}
struct TextDecoder {
    encoding: Encoding,
    bytes: Vec<u8>,
    odd: Option<u8>,
    truncated: bool,
}
impl TextDecoder {
    fn new(encoding: Encoding) -> Self {
        Self {
            encoding,
            bytes: vec![],
            odd: None,
            truncated: false,
        }
    }
    fn line(&mut self) -> String {
        let mut text = match self.encoding {
            Encoding::Utf8 => String::from_utf8_lossy(&self.bytes).into_owned(),
            encoding => {
                let units = self.bytes.as_chunks::<2>().0.iter().map(|b| {
                    if encoding == Encoding::Le {
                        u16::from_le_bytes([b[0], b[1]])
                    } else {
                        u16::from_be_bytes([b[0], b[1]])
                    }
                });
                char::decode_utf16(units)
                    .map(|r| r.unwrap_or('\u{fffd}'))
                    .collect()
            }
        };
        self.bytes.clear();
        text.truncate(text.trim_end_matches('\r').len());
        if text.len() > 65536 {
            let mut n = 65536;
            while !text.is_char_boundary(n) {
                n -= 1
            }
            text.truncate(n);
            self.truncated = true;
        }
        if self.truncated {
            text.push_str(" [line truncated]");
            self.truncated = false
        }
        if text.contains('\u{fffd}') {
            text.push_str(" [decoding replacement]")
        }
        text
    }
    fn feed(&mut self, data: &[u8]) -> Vec<String> {
        let mut lines = vec![];
        if self.encoding == Encoding::Utf8 {
            for segment in data.split_inclusive(|&b| b == b'\n') {
                let complete = segment.last() == Some(&b'\n');
                let text = if complete {
                    &segment[..segment.len() - 1]
                } else {
                    segment
                };
                let keep = text.len().min(65536 - self.bytes.len());
                self.bytes.extend_from_slice(&text[..keep]);
                self.truncated |= keep < text.len();
                if complete {
                    lines.push(self.line());
                }
            }
            return lines;
        }
        for &b in data {
            if let Some(first) = self.odd.take() {
                let unit = if self.encoding == Encoding::Le {
                    u16::from_le_bytes([first, b])
                } else {
                    u16::from_be_bytes([first, b])
                };
                if unit == 10 {
                    lines.push(self.line())
                } else if self.bytes.len() < 131072 {
                    self.bytes.extend_from_slice(&[first, b])
                } else {
                    self.truncated = true
                }
            } else {
                self.odd = Some(b)
            }
        }
        lines
    }
}
fn open(path: &str) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(7);
    }
    options.open(path)
}
#[cfg(windows)]
fn identity(file: &File) -> std::io::Result<(u64, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((
        info.dwVolumeSerialNumber as u64,
        ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
    ))
}
#[cfg(unix)]
fn identity(file: &File) -> std::io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata()?;
    Ok((m.dev(), m.ino()))
}
fn initial(file: &mut File, tail: bool) -> std::io::Result<(TextDecoder, Vec<String>, u64)> {
    let mut bom = [0u8; 3];
    file.seek(SeekFrom::Start(0))?;
    let n = file.read(&mut bom)?;
    let (encoding, skip) = if n >= 2 && bom[..2] == [255, 254] {
        (Encoding::Le, 2)
    } else if n >= 2 && bom[..2] == [254, 255] {
        (Encoding::Be, 2)
    } else if n >= 3 && bom == [239, 187, 191] {
        (Encoding::Utf8, 3)
    } else {
        (Encoding::Utf8, 0)
    };
    let len = file.metadata()?.len();
    let mut start = len.saturating_sub(16 * 1024 * 1024).max(skip);
    if encoding != Encoding::Utf8 {
        start -= start % 2
    }
    file.seek(SeekFrom::Start(start))?;
    let mut decoder = TextDecoder::new(encoding);
    let mut recent = std::collections::VecDeque::new();
    let mut recent_bytes = 0usize;
    let mut chunk = [0; 65536];
    let mut first = start > skip;
    let mut remaining = len.saturating_sub(start);
    while remaining > 0 {
        let count = file.read(&mut chunk[..remaining.min(65536) as usize])?;
        if count == 0 {
            break;
        }
        remaining -= count as u64;
        for line in decoder.feed(&chunk[..count]) {
            if first {
                first = false;
                continue;
            }
            recent_bytes += line.len();
            recent.push_back(line);
            while recent.len() > if tail { 200 } else { 20000 } || recent_bytes > 16 * 1024 * 1024 {
                recent_bytes -= recent.pop_front().unwrap().len();
            }
        }
    }
    let offset = file.stream_position()?;
    Ok((decoder, recent.into_iter().collect(), offset))
}
pub fn start(
    rt: &tokio::runtime::Runtime,
    source: Source,
    view: Arc<Mutex<View>>,
    mut cancel: oneshot::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    rt.spawn_blocking(move || {
        let mut current: Option<(File, (u64, u64), TextDecoder, u64)> = None;
        let mut first = true;
        let mut interrupted = false;
        loop {
            match cancel.try_recv() {
                Ok(_) | Err(oneshot::error::TryRecvError::Closed) => break,
                Err(oneshot::error::TryRecvError::Empty) => {}
            }
            let result: std::io::Result<()> = (|| {
                let mut candidate = open(&source.path)?;
                let id = identity(&candidate)?;
                let len = candidate.metadata()?.len();
                let replaced = current.as_ref().is_some_and(|(_, old, _, _)| *old != id);
                let truncated = current
                    .as_ref()
                    .is_some_and(|(_, _, _, offset)| len < *offset);
                if current.is_none() || replaced || truncated {
                    if !first {
                        consume(
                            &view,
                            vec![
                                if replaced {
                                    "[Local file replaced]"
                                } else if truncated {
                                    "[Local file truncated]"
                                } else {
                                    "[Local file available again]"
                                }
                                .into(),
                            ],
                            false,
                            &mut None,
                        );
                    }
                    let (decoder, lines, offset) = initial(&mut candidate, first || interrupted)?;
                    consume(&view, lines, false, &mut None);
                    current = Some((candidate, id, decoder, offset));
                    first = false;
                    interrupted = false;
                }
                if let Some((file, _, decoder, offset)) = &mut current {
                    let mut chunk = [0; 65536];
                    let mut budget = 4 * 1024 * 1024;
                    while budget > 0 {
                        let n = file.read(&mut chunk)?;
                        if n == 0 {
                            break;
                        }
                        *offset += n as u64;
                        budget -= n;
                        consume(&view, decoder.feed(&chunk[..n]), false, &mut None);
                    }
                }
                let mut v = view.lock().unwrap();
                v.set_status("Live · local");
                v.set_diagnostic(String::new());
                Ok(())
            })();
            if let Err(e) = result {
                let mut v = view.lock().unwrap();
                v.set_status("Waiting for local file");
                v.set_diagnostic(e.to_string());
                drop(v);
                if !interrupted {
                    consume(
                        &view,
                        vec![format!("[Local read interrupted: {e}]")],
                        false,
                        &mut None,
                    )
                }
                interrupted = true;
                current = None;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        view.lock().unwrap().set_status("Stopped");
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view() -> Arc<Mutex<View>> {
        Arc::new(Mutex::new(View::new(
            Default::default(),
            &Settings::default().rules,
        )))
    }
    fn until(mut f: impl FnMut() -> bool) {
        let start = std::time::Instant::now();
        while !f() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "Local reader timed out"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    #[test]
    fn file_lifecycle() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("sshlogtail-{}", new_id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("a file.log");
        std::fs::write(
            &path,
            (0..300).map(|i| format!("line {i}\n")).collect::<String>(),
        )
        .unwrap();
        let v = view();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (tx, rx) = oneshot::channel();
        start(
            &rt,
            Source {
                path: path.display().to_string(),
                kind: SourceKind::LocalFile,
                ..Default::default()
            },
            v.clone(),
            rx,
        );
        until(|| v.lock().unwrap().visible_len() == 200);
        assert_eq!(v.lock().unwrap().visible_line(0).text, "line 100");
        {
            let mut f = OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(b"partial ").unwrap();
        }
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(v.lock().unwrap().visible_len(), 200);
        {
            let mut f = OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all("é\n".as_bytes()).unwrap();
        }
        until(|| {
            v.lock()
                .unwrap()
                .visible_lines()
                .any(|l| l.text == "partial é")
        });
        std::fs::rename(&path, dir.join("old.log")).unwrap();
        std::fs::write(&path, b"replacement\n").unwrap();
        until(|| {
            v.lock()
                .unwrap()
                .visible_lines()
                .any(|l| l.text == "replacement")
        });
        std::fs::write(&path, b"short\n").unwrap();
        until(|| v.lock().unwrap().visible_lines().any(|l| l.text == "short"));
        std::fs::remove_file(&path).unwrap();
        until(|| v.lock().unwrap().status.contains("Waiting"));
        std::fs::write(&path, b"returned\n").unwrap();
        until(|| {
            v.lock()
                .unwrap()
                .visible_lines()
                .any(|l| l.text == "returned")
        });
        tx.send(()).unwrap();
        until(|| v.lock().unwrap().status == "Stopped");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(dir.join("old.log")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn access_error_and_cancellation() {
        use std::os::windows::fs::OpenOptionsExt;
        let path = std::env::temp_dir().join(format!("sshlogtail-locked-{}.log", new_id()));
        std::fs::write(&path, b"blocked\n").unwrap();
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let v = view();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (tx, rx) = oneshot::channel();
        start(
            &rt,
            Source {
                path: path.display().to_string(),
                kind: SourceKind::LocalFile,
                ..Default::default()
            },
            v.clone(),
            rx,
        );
        until(|| !v.lock().unwrap().diagnostic.is_empty());
        assert_eq!(v.lock().unwrap().status, "Waiting for local file");
        tx.send(()).unwrap();
        until(|| v.lock().unwrap().status == "Stopped");
        drop(locked);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn utf16_initial_tail() {
        let dir = std::env::temp_dir().join(format!("sshlogtail-{}", new_id()));
        std::fs::create_dir(&dir).unwrap();
        for (name, bom, enc) in [
            ("le", vec![255, 254], Encoding::Le),
            ("be", vec![254, 255], Encoding::Be),
        ] {
            let path = dir.join(name);
            let mut bytes = bom;
            for u in "hello 😀\nsecond\n".encode_utf16() {
                bytes.extend(if enc == Encoding::Le {
                    u.to_le_bytes()
                } else {
                    u.to_be_bytes()
                });
            }
            std::fs::write(&path, bytes).unwrap();
            let (_, lines, _) = initial(&mut open(path.to_str().unwrap()).unwrap(), true).unwrap();
            assert_eq!(lines, vec!["hello 😀", "second"]);
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn encodings_and_chunks() {
        for (enc, bytes) in [
            (Encoding::Utf8, "é😀\n".as_bytes().to_vec()),
            (
                Encoding::Le,
                "é😀\n".encode_utf16().flat_map(u16::to_le_bytes).collect(),
            ),
            (
                Encoding::Be,
                "é😀\n".encode_utf16().flat_map(u16::to_be_bytes).collect(),
            ),
        ] {
            let mut d = TextDecoder::new(enc);
            let mut out = vec![];
            for b in bytes {
                out.extend(d.feed(&[b]))
            }
            assert_eq!(out, vec!["é😀"]);
        }
    }
    #[test]
    fn invalid_and_oversized() {
        let mut d = TextDecoder::new(Encoding::Utf8);
        assert!(d.feed(&[255, 10])[0].contains("decoding replacement"));
        d.feed(&vec![b'x'; 70000]);
        assert!(d.feed(b"\n")[0].ends_with("[line truncated]"));
    }

    #[test]
    fn carriage_returns_and_invalid_utf16() {
        for enc in [Encoding::Utf8, Encoding::Le, Encoding::Be] {
            let text = "\n\r\r\nhello\rworld\r\r\né😀\npartial";
            let bytes: Vec<_> = match enc {
                Encoding::Utf8 => text.as_bytes().to_vec(),
                Encoding::Le => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
                Encoding::Be => text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
            };
            for size in [1, 3, bytes.len()] {
                let mut decoder = TextDecoder::new(enc);
                let lines: Vec<_> = bytes.chunks(size).flat_map(|c| decoder.feed(c)).collect();
                assert_eq!(lines, ["", "", "hello\rworld", "é😀"]);
            }
        }
        let mut decoder = TextDecoder::new(Encoding::Le);
        assert_eq!(
            decoder.feed(&[0, 0xd8, 10, 0]),
            ["� [decoding replacement]"]
        );
        let mut bytes = vec![b'x'; 65535];
        bytes.extend_from_slice("😀ignored\nnext\r\r\n".as_bytes());
        for size in [1, 3, 4096, bytes.len()] {
            let mut decoder = TextDecoder::new(Encoding::Utf8);
            let actual: Vec<_> = bytes.chunks(size).flat_map(|c| decoder.feed(c)).collect();
            // The local decoder bounds decoded UTF-8 too, so a replacement
            // character crossing the limit is removed before marker detection.
            assert_eq!(actual[0], format!("{} [line truncated]", "x".repeat(65535)));
            assert_eq!(actual[1], "next");
        }
    }

    #[test]
    #[ignore = "release performance measurement"]
    fn local_decode_benchmark() {
        let payload = format!("INFO payload={}\r\n", "x".repeat(180)).repeat(128);
        let mut decoder = TextDecoder::new(Encoding::Utf8);
        let start = std::time::Instant::now();
        for _ in 0..4000 {
            std::hint::black_box(decoder.feed(std::hint::black_box(payload.as_bytes())));
        }
        println!("LOCAL_BENCH seconds={:.6}", start.elapsed().as_secs_f64());
    }
}
