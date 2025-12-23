pub mod zstd {
    use std::io::{BufReader, Read, Result};

    use zstd::stream::read::{Decoder as ZstdDecoder, Encoder as ZstdEncoder};
    pub use zstd::DEFAULT_COMPRESSION_LEVEL;

    pub fn encoder<R: Read>(r: R, level: i32) -> Result<ZstdEncoder<'static, BufReader<R>>> {
        ZstdEncoder::new(r, level)
    }

    pub fn decoder<R: Read>(r: R) -> Result<ZstdDecoder<'static, BufReader<R>>> {
        ZstdDecoder::new(r)
    }
}

pub mod deflate {
    use std::io::Read;

    use flate2::read::{DeflateDecoder, DeflateEncoder};
    pub use flate2::Compression;

    pub fn encoder<R: Read>(r: R, level: Compression) -> DeflateEncoder<R> {
        DeflateEncoder::new(r, level)
    }

    pub fn decoder<R: Read>(r: R) -> DeflateDecoder<R> {
        DeflateDecoder::new(r)
    }
}

pub mod gz {
    use std::io::Read;

    use flate2::read::{GzDecoder, GzEncoder};
    pub use flate2::Compression;

    pub fn encoder<R: Read>(r: R, level: Compression) -> GzEncoder<R> {
        GzEncoder::new(r, level)
    }

    pub fn decoder<R: Read>(r: R) -> GzDecoder<R> {
        GzDecoder::new(r)
    }
}

pub mod zlib {
    use std::io::Read;

    use flate2::read::{ZlibDecoder, ZlibEncoder};
    pub use flate2::Compression;

    pub fn encoder<R: Read>(r: R, level: Compression) -> ZlibEncoder<R> {
        ZlibEncoder::new(r, level)
    }

    pub fn decoder<R: Read>(r: R) -> ZlibDecoder<R> {
        ZlibDecoder::new(r)
    }
}

pub mod brotli {
    use std::io::Read;

    use brotli::{CompressorReader as BrotliEncoder, Decompressor as BrotliDecoder};

    pub fn encoder<R: Read>(r: R) -> BrotliEncoder<R> {
        BrotliEncoder::new(r, 8_096, 11, 22)
    }

    pub fn decoder<R: Read>(r: R) -> BrotliDecoder<R> {
        BrotliDecoder::new(r, 8_096)
    }
}
