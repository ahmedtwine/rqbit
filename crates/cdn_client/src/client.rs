use librqbit::torrent_from_bytes;
use librqbit::{
    torrent_state::ManagedTorrentBuilder, AddTorrent, AddTorrentOptions, AddTorrentResponse,
    ManagedTorrent, PeerConnectionOptions, Session, SessionOptions,
};
use librqbit_core::torrent_metainfo::TorrentMetaV1Info;
use librqbit_core::Id20;
use serde_bencode;
use serde_bytes::ByteBuf;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tracing::Span;

struct CdnClient {
    db: SqlitePool,
    session: Arc<Session>,
    out_dir: PathBuf,
}

fn generate_torrent(chunks: &[Vec<u8>]) -> TorrentMetaV1Info<ByteBuf> {
    todo!("generate torrent from chunks")
}

async fn download_content(_url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    todo!()
}

impl CdnClient {
    async fn new(out_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let db = SqlitePool::connect("sqlite:cdn_cache.db").await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS torrents (
                url TEXT PRIMARY KEY,
                torrent_file BLOB,
                created_at INTEGER,
                expires_at INTEGER
            )",
        )
        .execute(&db)
        .await?;

        let sopts = SessionOptions {
            disable_dht: false,
            disable_dht_persistence: false,
            dht_config: None,
            persistence: false,
            persistence_filename: None,
            peer_id: None,
            peer_opts: Some(PeerConnectionOptions {
                connect_timeout: Some(Duration::from_secs(2)),
                read_write_timeout: Some(Duration::from_secs(10)),
                ..Default::default()
            }),
            listen_port_range: None,
            enable_upnp_port_forwarding: true,
        };

        let session = Session::new_with_opts(PathBuf::from("cdn_cache"), sopts)
            .await
            .map_err(|e| e.to_string())?;

        Ok(Self {
            db,
            session,
            out_dir,
        })
    }

    async fn fetch_content(
        &self,
        url: &str,
        chunk_size: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Check if a torrent exists for the given URL and if it hasn't expired
        let torrent_row =
            sqlx::query("SELECT torrent_file, expires_at FROM torrents WHERE url = ?")
                .bind(url)
                .fetch_optional(&self.db)
                .await?;

        if let Some(row) = torrent_row {
            let torrent_file: Vec<u8> = row.get(0);
            let expires_at: i64 = row.get(1);

            if expires_at > chrono::Utc::now().timestamp() {
                // Torrent exists and hasn't expired, use BitTorrent to fetch the content
                let torrent = torrent_from_bytes(&torrent_file)?;
                let torrent_info = torrent.info;
                let torrent_id = Id20::new([0; 20]); // Replace with actual torrent ID
                let torrent = ManagedTorrentBuilder::new(torrent_info, torrent_id, &self.out_dir)
                    .build(Span::current())?;
                self.fetch_content_from_torrent(torrent).await?;
                return Ok(());
            }
        }

        // Torrent doesn't exist or has expired, fetch from the upstream CDN
        let content = download_content(url).await?;

        // Chunk the content
        let chunks = content
            .chunks(chunk_size)
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>();

        // Generate the torrent file
        let torrent = generate_torrent(&chunks);
        let torrent_file = serde_bencode::to_bytes(&torrent)?;

        // Save the torrent to the database
        sqlx::query(
            "INSERT OR REPLACE INTO torrents (url, torrent_file, created_at, expires_at)
            VALUES (?, ?, ?, ?)",
        )
        .bind(url)
        .bind(&torrent_file)
        .bind(chrono::Utc::now().timestamp())
        .bind(chrono::Utc::now().timestamp() + 3600) // Expires in 1 hour
        .execute(&self.db)
        .await?;

        // Fetch the content using BitTorrent
        let torrent_id = Id20::from_str("00000fffffffffffffffffffffffffffffffffff").unwrap(); // Replace with actual torrent ID
        let torrent = ManagedTorrentBuilder::new(torrent.info, torrent_id, &self.out_dir)
            .build(Span::current())?;
        self.fetch_content_from_torrent(torrent).await?;

        Ok(())
    }

    async fn fetch_content_from_torrent(
        &self,
        torrent: Arc<ManagedTorrent>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let handle = self
            .session
            .add_torrent(
                AddTorrent::from_bytes(serde_bencode::to_bytes(&torrent.info().info)?),
                Some(AddTorrentOptions {
                    overwrite: true,
                    ..Default::default()
                }),
            )
            .await?;

        match handle {
            AddTorrentResponse::Added(_, handle) => {
                handle.wait_until_completed().await?;
                Ok(())
            }
            _ => Err("torrent already added or invalid".into()),
        }
    }

    async fn start_tracker_sync(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Expose a port for the tracker to send updates
        let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;

        loop {
            let (mut socket, _) = listener.accept().await?;
            let db = self.db.clone();

            tokio::spawn(async move {
                // Read the torrent file from the socket
                let mut torrent_file = ByteBuf::new();
                if let Err(e) = socket.read_to_end(&mut torrent_file).await {
                    eprintln!("Error reading torrent file from socket: {:?}", e);
                    return;
                }

                // Parse the torrent file
                let torrent_info = match torrent_from_bytes(&torrent_file) {
                    Ok(info) => info,
                    Err(e) => {
                        eprintln!("Error parsing torrent file: {:?}", e);
                        return;
                    }
                };

                // Save the torrent to the database
                if let Err(e) = sqlx::query(
                    "INSERT OR REPLACE INTO torrents (url, torrent_file, created_at, expires_at)
                    VALUES (?, ?, ?, ?)",
                )
                .bind(torrent_info.info_hash.as_string())
                .bind(&torrent_file)
                .bind(chrono::Utc::now().timestamp())
                .bind(chrono::Utc::now().timestamp() + 3600) // Expires in 1 hour
                .execute(&db)
                .await
                {
                    eprintln!("Error saving torrent to database: {:?}", e);
                }
            });
        }
    }
}
