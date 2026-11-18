//! Native transport smoke test: numeric loopback only, no challenge workflow.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

#[tokio::test]
async fn native_transport_reads_local_http_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "local client did not connect");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("local accept failed: {error}"),
            }
        };
        // Accepted Winsock sockets inherit the listener's nonblocking mode.
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
            assert!(request.len() <= 8192, "unexpectedly large local request");
        }
        assert!(request.starts_with(b"GET /build-smoke HTTP/1.1\r\n"));
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 17\r\nConnection: close\r\n\r\nlocal-build-smoke").unwrap();
    });
    let client = wreq::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let response = client
        .get(format!("http://{address}/build-smoke"))
        .send()
        .await;
    // Always wait for the bounded server thread, including client failures.
    server.join().unwrap();
    let response = response.unwrap();
    assert_eq!(response.status().as_u16(), 200);
    assert_eq!(response.text().await.unwrap(), "local-build-smoke");
}
