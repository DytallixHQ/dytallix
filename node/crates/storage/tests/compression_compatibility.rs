//! Exercise persisted SST codecs through the selected storage implementation.
use dytallix_storage::state::Storage;
use rocksdb::{DBCompressionType, Options, WriteOptions, DB};

fn reopen_compressed_sst(codec: DBCompressionType, codec_name: &str) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state");
    let value = vec![b'x'; 4096];
    let count = 256_u32;
    let files = {
        let mut options = Options::default();
        options.create_if_missing(true);
        options.set_compression_type(codec);
        options.set_disable_auto_compactions(true);
        let database = DB::open(&options, &path).unwrap();
        // Disable the WAL so a successful reopen must read flushed SST data.
        let mut writes = WriteOptions::default();
        writes.disable_wal(true);
        for index in 0..count {
            database
                .put_opt(index.to_be_bytes(), &value, &writes)
                .unwrap();
        }
        database.flush().unwrap();
        let files = database.live_files().unwrap();
        assert!(!files.is_empty(), "test must create an SST file");
        let total_size: usize = files.iter().map(|file| file.size).sum();
        assert!(
            total_size < count as usize * value.len() / 4,
            "test must write compressed data, not an uncompressed fallback"
        );
        let files: Vec<_> = files
            .into_iter()
            .map(|file| {
                let file_path = path.join(file.name.trim_start_matches('/'));
                let bytes = std::fs::read(&file_path).unwrap();
                assert!(
                    bytes
                        .windows(codec_name.len())
                        .any(|part| part == codec_name.as_bytes()),
                    "SST compression property must name the requested codec"
                );
                (file_path, bytes)
            })
            .collect();
        eprintln!(
            "codec={codec_name} rows={count} value_bytes={} sst_bytes={total_size}",
            value.len()
        );
        files
    };

    // Reopen twice through the same entry point used by the application.
    for _ in 0..2 {
        let storage = Storage::open(path.clone()).unwrap();
        for index in 0..count {
            assert_eq!(
                storage.db.get(index.to_be_bytes()).unwrap(),
                Some(value.clone())
            );
        }
        assert_eq!(
            storage.db.iterator(rocksdb::IteratorMode::Start).count(),
            count as usize
        );
        // Reading must not silently replace the original table with another codec.
        for (file_path, original) in &files {
            assert_eq!(&std::fs::read(file_path).unwrap(), original);
        }
    }
}

#[test]
fn selected_storage_reopens_snappy_sst_without_wal_replay() {
    reopen_compressed_sst(DBCompressionType::Snappy, "Snappy");
}

#[test]
fn selected_storage_reopens_lz4_sst_without_wal_replay() {
    reopen_compressed_sst(DBCompressionType::Lz4, "LZ4");
}
