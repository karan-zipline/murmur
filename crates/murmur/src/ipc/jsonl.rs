use std::io;

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

pub async fn write_jsonl<W, T>(writer: &mut W, value: &T) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let mut buf = serde_json::to_vec(value).map_err(invalid_data)?;
    buf.push(b'\n');
    writer.write_all(&buf).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_jsonl<R, T>(reader: &mut R) -> io::Result<Option<T>>
where
    R: AsyncBufRead + Unpin,
    T: DeserializeOwned,
{
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(None);
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value = serde_json::from_str(trimmed).map_err(invalid_data)?;
        return Ok(Some(value));
    }
}

fn invalid_data(err: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn jsonl_round_trip_two_messages() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let mut b = tokio::io::BufReader::new(&mut b);

        let writer = tokio::spawn(async move {
            write_jsonl(&mut a, &serde_json::json!({"n": 1}))
                .await
                .unwrap();
            write_jsonl(&mut a, &serde_json::json!({"n": 2}))
                .await
                .unwrap();
        });

        let first: serde_json::Value = read_jsonl(&mut b).await.unwrap().unwrap();
        let second: serde_json::Value = read_jsonl(&mut b).await.unwrap().unwrap();

        assert_eq!(first, serde_json::json!({"n": 1}));
        assert_eq!(second, serde_json::json!({"n": 2}));

        writer.await.unwrap();
    }

    #[tokio::test]
    async fn jsonl_skips_empty_lines() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let mut b = tokio::io::BufReader::new(&mut b);

        let writer = tokio::spawn(async move {
            a.write_all(b"\n\n").await.unwrap();
            write_jsonl(&mut a, &serde_json::json!({"ok": true}))
                .await
                .unwrap();
        });

        let value: serde_json::Value = read_jsonl(&mut b).await.unwrap().unwrap();
        assert_eq!(value, serde_json::json!({"ok": true}));

        writer.await.unwrap();
    }

    #[tokio::test]
    async fn jsonl_invalid_json_is_invalid_data() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let mut b = tokio::io::BufReader::new(&mut b);
        a.write_all(b"{not-json}\n").await.unwrap();

        let err = read_jsonl::<_, serde_json::Value>(&mut b)
            .await
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
