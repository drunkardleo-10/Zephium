//! Bound store response bodies before retaining them in memory.

pub(super) async fn read_body(
    mut response: reqwest::Response,
    limit: u64,
    oversized: &str,
    interrupted: &str,
) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(oversized.into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| interrupted.to_owned())? {
        // Content-Length may be absent. Check every chunk before copying it,
        // including the chunk that would cross the limit.
        if (chunk.len() as u64) > limit.saturating_sub(bytes.len() as u64) {
            return Err(oversized.into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::read_body;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    fn read_fixture(wire: &'static [u8], limit: u64) -> Result<Vec<u8>, String> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() < 8192);
            }
            socket.write_all(wire).unwrap();
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(async {
            let response = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .get(format!("http://{address}/"))
                .send()
                .await
                .unwrap();
            read_body(response, limit, "too large", "interrupted").await
        });
        server.join().unwrap();
        result
    }

    #[test]
    fn accepts_fixed_and_chunked_bodies_at_the_limit() {
        for wire in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n12345678".as_slice(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n1234\r\n4\r\n5678\r\n0\r\n\r\n".as_slice(),
        ] {
            assert_eq!(read_fixture(wire, 8).unwrap(), b"12345678");
        }
    }

    #[test]
    fn rejects_oversized_length_without_waiting_for_the_body() {
        assert_eq!(
            read_fixture(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n", 8),
            Err("too large".into())
        );
    }

    #[test]
    fn rejects_chunked_body_without_content_length() {
        assert_eq!(
            read_fixture(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n1234\r\n5\r\n56789\r\n0\r\n\r\n", 8),
            Err("too large".into())
        );
    }

    #[test]
    fn does_not_accept_a_truncated_download() {
        assert_eq!(
            read_fixture(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\n1234", 8),
            Err("interrupted".into())
        );
    }
}
