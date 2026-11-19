use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("无效的 LZ11/LZ13 数据: {0}")]
    InvalidLz11(String),

    #[error("无效的 IS .bin 归档: {0}")]
    InvalidArchive(String),

    #[error("数据越界: {0}")]
    OutOfBounds(String),

    #[error("Schema 解析或校验失败: {0}")]
    Schema(String),

    #[error("补丁特征码未命中: {0}")]
    PatternNotFound(String),

    #[error("成长率加解密表无效: {0}")]
    InvalidCipher(String),

    #[error("CTPK 贴图解码失败: {0}")]
    InvalidTexture(String),

    #[error("ROM 镜像解析失败: {0}")]
    InvalidRom(String),
}

pub type CoreResult<T> = Result<T, CoreError>;