pub mod local;
pub mod s3;

pub use local::LocalVaultStorage;
pub use s3::S3VaultStorage;
