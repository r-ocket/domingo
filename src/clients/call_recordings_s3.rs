//! S3-backed storage for call recordings (private bucket + presigned playback URLs)

use std::path::Path;
use std::time::Duration;

use aws_config::BehaviorVersion;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::types::ServerSideEncryption;
use aws_sdk_s3::Client as S3SdkClient;
use aws_sdk_s3::primitives::ByteStream;

#[derive(Clone)]
pub struct CallRecordingsS3 {
    client: S3SdkClient,
    bucket: String,
    prefix: String,
    presign_ttl: Duration,
}

#[derive(Debug, Clone)]
pub struct UploadedRecording {
    pub bucket: String,
    pub key: String,
    pub content_type: String,
}

impl CallRecordingsS3 {
    pub async fn new(bucket: String, prefix: String, presign_ttl_secs: u64) -> anyhow::Result<Self> {
        let cfg = aws_config::defaults(BehaviorVersion::latest()).load().await;
        let client = S3SdkClient::new(&cfg);
        Ok(Self {
            client,
            bucket,
            prefix,
            presign_ttl: Duration::from_secs(presign_ttl_secs.max(60)),
        })
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    pub fn key_for_call(&self, call_sid: &str) -> String {
        // stable, predictable key for easy ops/debugging
        // ex: calls/CA1234.wav
        let mut prefix = self.prefix.clone();
        if !prefix.is_empty() && !prefix.ends_with('/') {
            prefix.push('/');
        }
        format!("{prefix}{call_sid}.wav")
    }

    pub async fn upload_wav_path(&self, key: &str, wav_path: &Path) -> anyhow::Result<UploadedRecording> {
        let body = ByteStream::from_path(wav_path.to_path_buf()).await?;
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type("audio/wav")
            .server_side_encryption(ServerSideEncryption::Aes256)
            .body(body)
            .send()
            .await?;

        Ok(UploadedRecording {
            bucket: self.bucket.clone(),
            key: key.to_string(),
            content_type: "audio/wav".to_string(),
        })
    }

    pub async fn presign_get(&self, key: &str) -> anyhow::Result<String> {
        let presign_cfg = PresigningConfig::expires_in(self.presign_ttl)?;
        let req = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presign_cfg)
            .await?;
        Ok(req.uri().to_string())
    }
}


