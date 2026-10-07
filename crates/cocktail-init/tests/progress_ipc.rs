//! Exercise progress through the real init stdout pipe; no external network services.
use cocktail_shared::progress::ProgressEvent;
use serde_json::{Value, json};
use std::{collections::HashMap, io::Write, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{ChildStdin, ChildStdout, Command},
};

async fn send(stdin: &mut ChildStdin, id: u64, method: &str, params: Value) {
    let body =
        serde_json::to_vec(&json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))
            .unwrap();
    stdin
        .write_all(&(body.len() as u32).to_be_bytes())
        .await
        .unwrap();
    stdin.write_all(&body).await.unwrap();
    stdin.flush().await.unwrap();
}

async fn collect(
    stdout: &mut ChildStdout,
    jobs: &[(u64, &str)],
    failed: bool,
) -> HashMap<String, Vec<(String, ProgressEvent)>> {
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut responses = std::collections::HashSet::new();
        let mut events: HashMap<String, Vec<(String, ProgressEvent)>> = HashMap::new();
        loop {
            let bytes = cocktail_init::server::read_frame(stdout).await.unwrap();
            let frame: Value = serde_json::from_slice(&bytes).unwrap();
            if let Some(id) = frame["id"].as_u64() {
                assert!(jobs.iter().any(|job| job.0 == id));
                assert_eq!(
                    !frame["error"].is_null(),
                    failed,
                    "unexpected response: {frame}"
                );
                responses.insert(id);
            } else if let Some(method) = frame["method"].as_str() {
                let progress: ProgressEvent =
                    serde_json::from_value(frame["params"].clone()).unwrap();
                assert!(jobs.iter().any(|job| job.1 == progress.transfer.id));
                let log = events.entry(progress.transfer.id.clone()).or_default();
                if let Some((previous, old)) = log.last() {
                    assert!(!previous.ends_with("completed") && !previous.ends_with("failed"));
                    assert!(progress.received >= old.received);
                    assert_eq!(progress.transfer.label, old.transfer.label);
                } else {
                    assert!(method.ends_with("started"));
                }
                log.push((method.into(), progress));
            }
            if responses.len() == jobs.len()
                && jobs.iter().all(|job| {
                    events.get(job.1).is_some_and(|log| {
                        log.last().is_some_and(|(method, _)| {
                            method.ends_with(if failed { "failed" } else { "completed" })
                        })
                    })
                })
            {
                break events;
            }
        }
    })
    .await
    .expect("progress or RPC response missing")
}

#[tokio::test]
async fn downloads_and_archives_report_distinct_jobs_and_terminal_events() {
    let dir = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_cocktail-init"));
    command
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .env("NO_PROXY", "127.0.0.1,localhost");
    for name in [
        "COCKTAIL_PROXY",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env_remove(name);
    }
    // Disable Windows system-proxy discovery; HTTP loopback remains direct.
    command.env("HTTPS_PROXY", "http://127.0.0.1:9");
    cocktail_shared::wincompat::hide_console(&mut command);
    let mut child = command.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/same-url", listener.local_addr().unwrap());
    let source = tokio::spawn(async move {
        let mut workers = tokio::task::JoinSet::new();
        for known_length in [true, false] {
            let (mut socket, _) = listener.accept().await.unwrap();
            workers.spawn(async move {
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    request.push(socket.read_u8().await.unwrap());
                }
                let header = if known_length {
                    "HTTP/1.0 200 OK\r\nContent-Length: 32768\r\n\r\n"
                } else {
                    "HTTP/1.0 200 OK\r\nConnection: close\r\n\r\n"
                };
                socket.write_all(header.as_bytes()).await.unwrap();
                for _ in 0..4 {
                    socket.write_all(&vec![b'x'; 8192]).await.unwrap();
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                socket.shutdown().await.unwrap();
            });
        }
        while let Some(result) = workers.join_next().await {
            result.unwrap();
        }
    });
    for (id, task) in [(1, "download-a"), (2, "download-b")] {
        send(
            &mut stdin,
            id,
            "http.download_to_path",
            json!({"url":url, "dest":dir.path().join(task),
            "progress":{"id":task, "label":"same-file"}}),
        )
        .await;
    }
    let downloads = collect(&mut stdout, &[(1, "download-a"), (2, "download-b")], false).await;
    for (task, log) in &downloads {
        assert!(log.iter().any(|(method, p)| method == "download.progress"
            && p.received > 0
            && p.received < 32768));
        assert_eq!(log.last().unwrap().1.received, 32768);
        assert_eq!(
            std::fs::metadata(dir.path().join(task)).unwrap().len(),
            32768
        );
    }
    assert!(downloads.values().any(|log| {
        log.iter()
            .any(|(method, p)| method == "download.progress" && p.received > 0 && p.total.is_none())
    }));
    source.await.unwrap();
    send(
        &mut stdin,
        3,
        "http.download_to_path",
        json!({"url":"not-a-url", "dest":dir.path().join("invalid"),
        "progress":{"id":"download-failed", "label":"invalid"}}),
    )
    .await;
    collect(&mut stdout, &[(3, "download-failed")], true).await;

    let archive = dir.path().join("pack.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    zip.start_file("server.jar", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"fixture server bytes").unwrap();
    zip.finish().unwrap();
    send(
        &mut stdin,
        4,
        "archive.extract_pack",
        json!({"archive_path":archive,
        "workdir":dir.path().join("instance"), "filename":"pack.zip",
        "progress":{"id":"extract-ok", "label":"pack.zip"}}),
    )
    .await;
    let extraction = collect(&mut stdout, &[(4, "extract-ok")], false).await;
    let stages: Vec<_> = extraction["extract-ok"]
        .iter()
        .filter_map(|(_, p)| p.stage.as_deref())
        .collect();
    assert!(stages.contains(&"extract") && stages.contains(&"merge"));
    assert_eq!(
        std::fs::read(dir.path().join("instance/server.jar")).unwrap(),
        b"fixture server bytes"
    );
    assert!(extraction["extract-ok"].last().unwrap().1.received > 0);
    std::fs::write(&archive, b"invalid zip").unwrap();
    send(
        &mut stdin,
        5,
        "archive.extract_pack",
        json!({"archive_path":archive,
        "workdir":dir.path().join("bad"), "filename":"pack.zip",
        "progress":{"id":"extract-failed", "label":"bad.zip"}}),
    )
    .await;
    let failed = collect(&mut stdout, &[(5, "extract-failed")], true).await;
    assert!(failed["extract-failed"].last().unwrap().1.error.is_some());
    let tar_path = dir.path().join("pack.tar");
    let mut tar = tar::Builder::new(std::fs::File::create(&tar_path).unwrap());
    let data = b"tar fixture";
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "server.jar", &data[..])
        .unwrap();
    tar.finish().unwrap();
    drop(tar);
    send(
        &mut stdin,
        6,
        "archive.extract_pack",
        json!({"archive_path":tar_path,
        "workdir":dir.path().join("tar-instance"), "filename":"pack.tar",
        "progress":{"id":"extract-tar", "label":"pack.tar"}}),
    )
    .await;
    let tar_events = collect(&mut stdout, &[(6, "extract-tar")], false).await;
    assert!(
        tar_events["extract-tar"]
            .iter()
            .any(|(method, p)| method == "extract.progress" && p.received > 0 && p.total.is_none())
    );
    assert_eq!(
        std::fs::read(dir.path().join("tar-instance/server.jar")).unwrap(),
        data
    );
    drop(stdin);
    assert!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}
