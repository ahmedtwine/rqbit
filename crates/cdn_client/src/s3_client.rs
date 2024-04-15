use aws_config::BehaviorVersion;
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::{error::SdkError, Client};
use reqwest;
use std::str;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

pub async fn download_stream_public_url(url: &str, file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let parts = 4; // Number of concurrent requests
    let mut handles = vec![];
    let file = Arc::new(Mutex::new(tokio::fs::File::create(file_path).await?));

    for part in 0..parts {
        let client = client.clone();
        let file = Arc::clone(&file);
        let url = url.to_string();
        let handle = tokio::spawn(async move {
            let start_byte = part * (1_500_000 / parts);
            let end_byte = ((part + 1) * (1_500_000 / parts)) - 1;
            let range_header_value = format!("bytes={}-{}", start_byte, end_byte);

            let response = client
                .get(&url)
                .header("Range", range_header_value)
                .send()
                .await
                .unwrap();

            let mut buffer = Vec::new();
            buffer.extend_from_slice(&response.bytes().await.unwrap());

            let mut file = file.lock().await;
            file.write_all(&buffer).await.unwrap();
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.await?;
    }

    Ok(())
}

pub async fn download_object(
    client: &Client,
    bucket_name: &str,
    key: &str,
    window: Option<tauri::Window>,
) -> Result<(), SdkError<GetObjectError>> {
    let mut stream = client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await?
        .body
        .into_async_read();

    let mut buffer = Vec::new();
    while stream.read_buf(&mut buffer).await.unwrap() != 0 {
        println!("Read {} bytes", buffer.len());
        if let Some(window) = window.as_ref() {
            window.emit("video-chink", buffer.clone()).unwrap();
        }
        buffer.clear();
    }

    Ok(())
}

pub async fn get_aws_client_bucket() -> (Client, &'static str) {
    let config = aws_config::load_defaults(BehaviorVersion::v2023_11_09()).await;
    let client = aws_sdk_s3::Client::new(&config);
    let bucket_name = "bacbone-assets";
    (client, bucket_name)
}
