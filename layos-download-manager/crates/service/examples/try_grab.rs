//! Runs the site grabber on a real site, as the app's "Grab files from a site" does.
//!
//!     cargo run -p ldm-service --example try_grab -- <start url> <depth> <types, e.g. zip,pdf>

use ldm_service::grabber::{GrabOptions, grab};

#[tokio::main]
async fn main() {
    let mut a = std::env::args().skip(1);
    let start_url = a.next().expect("start url");
    let depth = a.next().map(|d| d.parse().unwrap()).unwrap_or(1);
    let file_types = a.next().unwrap_or_else(|| "zip,pdf".into()).split(',').map(str::to_string).collect();
    let client = reqwest::Client::builder()
        .user_agent(concat!("LayosDownloadManager/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap();
    let started = std::time::Instant::now();
    let opts = GrabOptions { start_url, depth, file_types, same_site: true, max_pages: 150 };
    match grab(&client, &opts).await {
        Ok(r) => {
            println!(
                "{} files from {} pages in {:.1}s (robots skipped {}, stopped early: {})",
                r.files.len(),
                r.pages_read,
                started.elapsed().as_secs_f32(),
                r.pages_skipped_by_robots,
                r.stopped_early
            );
            for f in r.files.iter().take(12) {
                println!("  {}  <- {}", f.file_name, f.page_url);
            }
        }
        Err(e) => println!("FAILED: {e}"),
    }
}
