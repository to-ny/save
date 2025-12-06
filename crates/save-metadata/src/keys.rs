//! Key prefix constants for RocksDB.
//!
//! All metadata keys use prefixes to partition the keyspace:
//! - `bkt:` - Bucket metadata
//! - `obj:` - Object metadata
//! - `mpu:` - Multipart upload metadata

pub const BUCKET_PREFIX: &str = "bkt:";
pub const OBJECT_PREFIX: &str = "obj:";
pub const MULTIPART_PREFIX: &str = "mpu:";

/// All data prefixes used in the default column family.
/// Used by snapshot restore to copy data between databases.
pub const DATA_PREFIXES: [&str; 3] = [BUCKET_PREFIX, OBJECT_PREFIX, MULTIPART_PREFIX];
