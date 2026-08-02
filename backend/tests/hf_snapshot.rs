use mockito::Server;
use quantamind_lib::inference::hf::hf_browse::HfRepoFile;
use quantamind_lib::inference::hf::hf_snapshot::{download_snapshot, SnapshotProgress};
use std::cell::Cell;
use std::fs;
use tokio_util::sync::CancellationToken;

fn body(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i & 0xff) as u8).collect()
}

fn file(path: &str, size: u64) -> HfRepoFile {
    HfRepoFile { path: path.into(), size_bytes: size }
}

#[tokio::test]
async fn downloads_every_file_into_dest_dir_with_aggregate_progress() {
    let mut s = Server::new_async().await;
    // config.json (100), model.safetensors (300), nested/tokenizer.json (50).
    for (p, n) in [("config.json", 100usize), ("model.safetensors", 300), ("nested/tokenizer.json", 50)] {
        let url = format!("/some-org/X/resolve/main/{p}");
        s.mock("HEAD", url.as_str()).with_status(200).with_header("content-length", &n.to_string()).create_async().await;
        s.mock("GET", url.as_str()).with_status(200).with_body(body(n)).create_async().await;
    }
    let dir = tempfile::tempdir().unwrap();
    let files = vec![file("config.json", 100), file("model.safetensors", 300), file("nested/tokenizer.json", 50)];
    let max = Cell::new(0u64);
    download_snapshot(&s.url(), "some-org/X", &files, dir.path(), |p: SnapshotProgress| {
        assert_eq!(p.bytes_total, 450);
        if p.bytes_completed > max.get() { max.set(p.bytes_completed); }
    }, CancellationToken::new()).await.expect("snapshot ok");

    assert_eq!(fs::read(dir.path().join("config.json")).unwrap(), body(100));
    assert_eq!(fs::read(dir.path().join("model.safetensors")).unwrap(), body(300));
    // Nested path created.
    assert_eq!(fs::read(dir.path().join("nested/tokenizer.json")).unwrap(), body(50));
    assert_eq!(max.get(), 450, "aggregate progress reaches the total");
}

#[tokio::test]
async fn an_already_finished_file_is_skipped() {
    let mut s = Server::new_async().await;
    // config.json already on disk → its GET must NOT be hit.
    let cfg = s.mock("GET", "/some-org/X/resolve/main/config.json").expect(0).create_async().await;
    let url = "/some-org/X/resolve/main/model.safetensors";
    s.mock("HEAD", url).with_status(200).with_header("content-length", "300").create_async().await;
    s.mock("GET", url).with_status(200).with_body(body(300)).create_async().await;

    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("config.json"), body(100)).unwrap();
    let files = vec![file("config.json", 100), file("model.safetensors", 300)];
    download_snapshot(&s.url(), "some-org/X", &files, dir.path(), |_| {}, CancellationToken::new())
        .await.expect("snapshot ok");

    assert_eq!(fs::read(dir.path().join("model.safetensors")).unwrap(), body(300));
    cfg.assert_async().await; // confirms the finished file's GET was skipped
}
