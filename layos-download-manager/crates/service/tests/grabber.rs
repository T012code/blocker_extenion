use ldm_service::grabber::{GrabOptions, grab};
use ldm_testkit::StaticServer;

fn site(dir: &std::path::Path) {
    let w = |p: &str, s: &str| {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, s).unwrap();
    };
    w(
        "index.html",
        r#"<a href="docs/">Docs</a> <a href="report.pdf">Report</a> <a href="https://elsewhere.example/x.pdf">Off-site</a> <a href="private/">P</a>"#,
    );
    w(
        "docs/index.html",
        r#"<a href="guide.pdf">Guide</a> <a href="../archive/">Archive</a> <a href="data.zip">Data</a>"#,
    );
    w("archive/index.html", r#"<a href="old.pdf">Old</a>"#);
    w("private/index.html", r#"<a href="secret.pdf">Secret</a>"#);
    w("robots.txt", "User-agent: *\nDisallow: /private\n");
    for f in [
        "report.pdf",
        "docs/guide.pdf",
        "docs/data.zip",
        "archive/old.pdf",
        "private/secret.pdf",
    ] {
        w(f, "%PDF");
    }
}

fn names(r: &ldm_service::grabber::GrabResult) -> Vec<String> {
    let mut n: Vec<String> = r.files.iter().map(|f| f.file_name.clone()).collect();
    n.sort();
    n
}

#[tokio::test]
async fn finds_files_by_depth_and_type_and_honours_robots() {
    let dir = tempfile::tempdir().unwrap();
    site(dir.path());
    let server = StaticServer::start(dir.path().to_path_buf()).await;
    let client = ldm_engine::http::build_client().unwrap();
    let opts = |depth, types: &[&str]| GrabOptions {
        start_url: server.url("index.html"),
        depth,
        file_types: types.iter().map(|s| s.to_string()).collect(),
        same_site: true,
        max_pages: 50,
    };

    let r = grab(&client, &opts(0, &["pdf"])).await.unwrap();
    assert_eq!(names(&r), ["report.pdf"], "depth 0: only the starting page");

    let r = grab(&client, &opts(1, &["pdf", "zip"])).await.unwrap();
    assert_eq!(names(&r), ["data.zip", "guide.pdf", "report.pdf"]);
    assert_eq!(
        r.pages_skipped_by_robots, 1,
        "the private folder is left alone"
    );
    assert!(
        r.files.iter().all(|f| !f.url.contains("elsewhere")),
        "other sites are skipped"
    );
    let guide = r.files.iter().find(|f| f.file_name == "guide.pdf").unwrap();
    assert!(
        guide.page_url.ends_with("/docs/"),
        "the referring page is kept: {}",
        guide.page_url
    );

    let r = grab(&client, &opts(2, &["pdf"])).await.unwrap();
    assert_eq!(names(&r), ["guide.pdf", "old.pdf", "report.pdf"]);

    let mut capped = opts(3, &["pdf"]);
    capped.max_pages = 1;
    let r = grab(&client, &capped).await.unwrap();
    assert!(r.stopped_early);
    assert_eq!(r.pages_read, 1);

    assert!(grab(&client, &opts(1, &[])).await.is_err());
}
