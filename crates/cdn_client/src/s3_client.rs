use aws_config::BehaviorVersion;
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::{error::SdkError, Client};
use std::str;
use tokio::io::AsyncReadExt;

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
