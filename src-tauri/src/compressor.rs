use anyhow::{Context, Result};

const DEFAULT_COMPRESSION_LEVEL: i32 = 3;

pub struct Compressor {
    level: i32,
}

impl Default for Compressor {
    fn default() -> Self {
        Self {
            level: DEFAULT_COMPRESSION_LEVEL,
        }
    }
}

impl Compressor {
    pub fn new(level: i32) -> Self {
        Self {
            level: level.clamp(1, 22),
        }
    }

    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        zstd::encode_all(data, self.level).context("Zstd compression failed")
    }

    pub fn decompress(&self, compressed: &[u8]) -> Result<Vec<u8>> {
        zstd::decode_all(compressed).context("Zstd decompression failed")
    }

    pub fn compress_multiple(blocks: &[Vec<u8>]) -> Result<Vec<u8>> {
        let compressor = Compressor::default();
        let mut result = Vec::new();
        let count = blocks.len() as u32;
        result.extend_from_slice(&count.to_le_bytes());

        for block in blocks {
            let compressed = compressor.compress(block)?;
            let size = compressed.len() as u32;
            result.extend_from_slice(&size.to_le_bytes());
            result.extend_from_slice(&compressed);
        }

        Ok(result)
    }

    pub fn decompress_multiple(data: &[u8]) -> Result<Vec<Vec<u8>>> {
        let compressor = Compressor::default();
        if data.len() < 4 {
            anyhow::bail!("Insufficient data for block count");
        }
        let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        let mut offset = 4;
        let mut blocks = Vec::with_capacity(count);

        for _ in 0..count {
            if data.len() < offset + 4 {
                anyhow::bail!("Insufficient data for block size");
            }
            let size = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            if data.len() < offset + size {
                anyhow::bail!("Insufficient data for block content");
            }
            let block = compressor.decompress(&data[offset..offset + size])?;
            blocks.push(block);
            offset += size;
        }

        Ok(blocks)
    }

    pub fn decompress_blocks(combined: &[u8]) -> Result<Vec<Vec<u8>>> {
        Self::decompress_multiple(combined)
    }
}
