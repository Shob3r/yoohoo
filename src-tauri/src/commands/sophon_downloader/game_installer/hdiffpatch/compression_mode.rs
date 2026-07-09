#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum CompressionMode {
    #[default]
    Nocomp,
    Zstd,
    Zlib,
    Lz4,
}

impl std::str::FromStr for CompressionMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "" | "nocomp" => Ok(CompressionMode::Nocomp),
            "zstd" => Ok(CompressionMode::Zstd),
            "zlib" => Ok(CompressionMode::Zlib),
            "lz4" => Ok(CompressionMode::Lz4),
            _ => Err(format!("unsupported compression mode: {s}")),
        }
    }
}
