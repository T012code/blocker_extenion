//! Downloads a video or audio stream with yt-dlp, reporting progress to the engine.
//!
//! Pausing stops yt-dlp; its partial files stay in a per-download work folder, and
//! continuing picks them up (`--continue`), so finished pieces aren't fetched again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Weak};

use futures_util::future::BoxFuture;
use ldm_engine::{Download, DownloadKind, DownloadStatus, Error, Job, Result, Runner};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::tools::{self, CookieFile, Tools};

const PROGRESS: &str = "LDMPROG";
const DONE: &str = "LDMDONE";

pub struct MediaRunner {
    tools: Tools,
    /// Partial files live here, one folder per download.
    work_dir: PathBuf,
    me: Weak<Self>,
}

impl MediaRunner {
    pub fn new(tools: Tools, work_dir: impl Into<PathBuf>) -> Arc<Self> {
        let work_dir = work_dir.into();
        Arc::new_cyclic(|me| Self {
            tools,
            work_dir,
            me: me.clone(),
        })
    }

    fn work_folder(&self, id: i64) -> PathBuf {
        self.work_dir.join(id.to_string())
    }

    async fn run_job(self: Arc<Self>, job: Job) -> Result<()> {
        let d = job.download.clone();
        let DownloadKind::Media(spec) = &d.kind else {
            return Err(Error::Other("not a media download".into()));
        };
        let ytdlp = self.tools.ytdlp()?.to_path_buf();
        let work = self.work_folder(d.id);
        std::fs::create_dir_all(&work)?;
        std::fs::create_dir_all(&d.save_dir)?;

        let mut req = d.request.clone();
        req.url = spec.page_url.clone();
        let cookies = CookieFile::write(&req)?;
        let mut cmd = tools::command(&ytdlp);
        cmd.args(["--newline", "--no-playlist", "--no-warnings", "--no-colors", "--continue", "--no-mtime"])
            .args(["--windows-filenames", "--no-simulate", "--progress"])
            .arg("-f")
            .arg(spec.format.as_deref().unwrap_or("bv*+ba/b"))
            .args(["--merge-output-format", "mp4/mkv"])
            // Several pieces of a stream at once, like sections of a file.
            .arg("--concurrent-fragments")
            .arg(d.connections.clamp(1, 16).to_string())
            .arg("-P")
            .arg(&d.save_dir)
            .arg("-P")
            .arg(format!("temp:{}", work.display()))
            .arg("-o")
            .arg(output_template(spec.title.as_deref()))
            .arg("--progress-template")
            .arg(format!(
                "download:{PROGRESS} %(info.format_id)s %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.speed)s"
            ))
            .arg("--print")
            .arg(format!("after_move:{DONE} %(filepath)s"));
        if let Some(ffmpeg) = &self.tools.ffmpeg {
            cmd.arg("--ffmpeg-location").arg(ffmpeg);
        }
        if d.speed_limit > 0 {
            cmd.arg("-r").arg(d.speed_limit.to_string());
        }
        tools::add_request_args(&mut cmd, &req, cookies.as_ref());
        cmd.arg("--").arg(&spec.page_url);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| Error::Other(format!("could not start yt-dlp: {e}")))?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let errors = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut all = String::new();
            while let Ok(Some(l)) = lines.next_line().await {
                all.push_str(&l);
                all.push('\n');
            }
            all
        });

        job.report.set_status(DownloadStatus::Downloading);
        let mut lines = BufReader::new(stdout).lines();
        // Each format (e.g. video, then audio) reports its own byte counts.
        let mut parts: HashMap<String, (u64, Option<u64>)> = HashMap::new();
        let mut final_path: Option<PathBuf> = None;
        loop {
            tokio::select! {
                _ = job.cancel.cancelled() => {
                    let _ = child.kill().await;
                    return Err(Error::Cancelled);
                }
                line = lines.next_line() => match line? {
                    Some(l) => {
                        if let Some(rest) = l.strip_prefix(PROGRESS) {
                            if let Some((fid, done, total, speed)) = parse_progress(rest) {
                                parts.insert(fid, (done, total));
                                let downloaded = parts.values().map(|p| p.0).sum();
                                let size = parts.values().map(|p| p.1).sum::<Option<u64>>();
                                job.report.progress(downloaded, size, speed);
                            }
                        } else if let Some(p) = l.strip_prefix(DONE) {
                            final_path = Some(PathBuf::from(p.trim()));
                        }
                    }
                    None => break,
                },
            }
        }
        let status = child.wait().await?;
        let stderr = errors.await.unwrap_or_default();
        if !status.success() {
            return Err(Error::Other(tools::last_error(&stderr)));
        }
        let path = final_path
            .ok_or_else(|| Error::Other("yt-dlp finished without saving a file".into()))?;
        let size = std::fs::metadata(&path).map(|m| m.len()).ok();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string);
        let dir = path.parent().map(Path::to_path_buf);
        job.report.update(|rec| {
            if let Some(n) = name {
                rec.file_name = n;
            }
            if let Some(dir) = dir {
                rec.save_dir = dir;
            }
            rec.size = size;
            rec.downloaded = size.unwrap_or(rec.downloaded);
        });
        let _ = std::fs::remove_dir_all(&work);
        Ok(())
    }
}

/// The file name: the page's title when the browser sent one (stream playlists often
/// have meaningless names like "index"), otherwise the title the site reports.
fn output_template(title: Option<&str>) -> String {
    match title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => {
            let safe = ldm_engine::filename::sanitize(t)
                .chars()
                .take(150)
                .collect::<String>();
            // % starts a field in yt-dlp templates.
            format!("{}.%(ext)s", safe.replace('%', "%%"))
        }
        None => "%(title).150B.%(ext)s".to_string(),
    }
}

/// `<format_id> <downloaded> <total> <estimate> <speed>`; numbers may be "NA".
fn parse_progress(rest: &str) -> Option<(String, u64, Option<u64>, u64)> {
    let mut it = rest.split_whitespace();
    let fid = it.next()?.to_string();
    let num = |s: Option<&str>| s.and_then(|v| v.parse::<f64>().ok()).map(|v| v as u64);
    let done = num(it.next())?;
    let total = num(it.next());
    let estimate = num(it.next());
    let speed = num(it.next()).unwrap_or(0);
    Some((fid, done, total.or(estimate), speed))
}

impl Runner for MediaRunner {
    fn run(&self, job: Job) -> BoxFuture<'static, Result<()>> {
        match self.me.upgrade() {
            Some(me) => Box::pin(me.run_job(job)),
            None => {
                Box::pin(async { Err(Error::Other("the video downloader has shut down".into())) })
            }
        }
    }

    fn removed(&self, download: &Download, delete_files: bool) {
        let _ = std::fs::remove_dir_all(self.work_folder(download.id));
        if delete_files && download.status == DownloadStatus::Completed {
            let _ = std::fs::remove_file(download.final_path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_files_after_the_page() {
        assert_eq!(
            output_template(Some("Live: show 100%")),
            "Live_ show 100%%.%(ext)s"
        );
        assert_eq!(output_template(Some("  ")), "%(title).150B.%(ext)s");
        assert_eq!(output_template(None), "%(title).150B.%(ext)s");
    }

    #[test]
    fn progress_lines() {
        assert_eq!(
            parse_progress(" 137 1024 4096 NA 2048.5"),
            Some(("137".into(), 1024, Some(4096), 2048))
        );
        assert_eq!(
            parse_progress(" hls-1 50 NA 999.2 NA"),
            Some(("hls-1".into(), 50, Some(999), 0))
        );
        assert_eq!(parse_progress(" x NA NA NA NA"), None);
    }
}
