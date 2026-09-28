//! Reuse connections across dictation and assistant requests. Construct this
//! client on a worker: reqwest's blocking client owns a runtime thread.
use std::sync::OnceLock;

use reqwest::blocking::Client;

pub(crate) fn client() -> Result<&'static Client, String> {
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .user_agent(concat!("Whisple/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(std::time::Duration::from_secs(15))
                .build()
                .map_err(|err| err.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    #[test]
    fn consecutive_requests_reuse_the_same_connection() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", server.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            for _ in 0..2 {
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                }
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                    .unwrap();
            }
        });
        for _ in 0..2 {
            assert_eq!(
                client()
                    .unwrap()
                    .get(&url)
                    .timeout(Duration::from_secs(3))
                    .send()
                    .unwrap()
                    .text()
                    .unwrap(),
                "ok"
            );
        }
        handle.join().unwrap();
    }
}
