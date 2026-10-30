//! Messages and framing. Each frame is a little-endian `u32` length, then that many bytes of JSON.

use quarry_driver_api::{DriverError, ExecItem, ExecOpts, RowBatch};
use quarry_model::{
    Capabilities, ConnectionConfig, DialectSpec, DriverId, ErrorLocation, SchemaChange,
    SchemaObject, ServerInfoKind, ServerInfoRow, TableColumns, TableInfo, TableRef, Value,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Bumped on any change a driver built for the previous number would misread.
pub const PROTOCOL: u32 = 2;

/// Row batches of a `Stream` call the driver may send ahead of the host's `Ack`s: enough to keep
/// the pipe busy, few enough that a slow consumer holds the database back.
pub const STREAM_WINDOW: usize = 4;

/// A frame larger than this is garbage, not a message: refuse it before allocating.
const MAX_FRAME: u32 = 512 * 1024 * 1024;

/// What a driver binary prints for `hello`: the drivers it serves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    /// The Quarry version the binary was built from.
    pub version: String,
    pub drivers: Vec<DriverInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriverInfo {
    pub id: DriverId,
    pub capabilities: Capabilities,
    pub dialect: DialectSpec,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ToDriver {
    Call {
        id: u64,
        call: Box<Call>,
    },
    /// Cancels the running call through the driver's cancel handle. Never waits behind it.
    Cancel,
    /// The host took one batch of rows of `Stream` call `id`: the driver may send another.
    Ack {
        id: u64,
    },
    /// The host wants no more of `Stream` call `id`: the driver stops reading and replies.
    Stop {
        id: u64,
    },
}

/// One method of `Driver` or `Session`.
#[derive(Serialize, Deserialize)]
pub enum Call {
    Connect {
        config: ConnectionConfig,
        password: Option<String>,
    },
    SchemaChange {
        change: SchemaChange,
    },
    Execute {
        sql: String,
        opts: ExecOpts,
    },
    ExecuteParams {
        sql: String,
        params: Vec<Value>,
    },
    Query {
        sql: String,
        params: Vec<Value>,
        opts: ExecOpts,
    },
    /// Every row, paced by the host: at most `STREAM_WINDOW` batches are sent before an `Ack`.
    Stream {
        sql: String,
        params: Vec<Value>,
        opts: ExecOpts,
    },
    ListDatabases,
    ListObjects {
        database: String,
    },
    ListColumns {
        database: String,
    },
    TableInfo {
        table: TableRef,
    },
    PrimaryKey {
        table: TableRef,
    },
    ServerInfo {
        kind: ServerInfoKind,
    },
    UseDatabase {
        database: String,
    },
    Begin,
    Commit,
    Rollback,
    SetReadOnly {
        read_only: bool,
    },
    SetStrict,
    Close,
}

// Never print the password, nor the SQL and values a user runs.
impl std::fmt::Debug for Call {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Connect { config, password } => {
                return f
                    .debug_struct("Connect")
                    .field("config", config)
                    .field("password", &password.as_ref().map(|_| "***"))
                    .finish();
            }
            Self::SchemaChange { .. } => "SchemaChange",
            Self::Execute { .. } => "Execute",
            Self::ExecuteParams { .. } => "ExecuteParams",
            Self::Query { .. } => "Query",
            Self::Stream { .. } => "Stream",
            Self::ListDatabases => "ListDatabases",
            Self::ListObjects { .. } => "ListObjects",
            Self::ListColumns { .. } => "ListColumns",
            Self::TableInfo { .. } => "TableInfo",
            Self::PrimaryKey { .. } => "PrimaryKey",
            Self::ServerInfo { .. } => "ServerInfo",
            Self::UseDatabase { .. } => "UseDatabase",
            Self::Begin => "Begin",
            Self::Commit => "Commit",
            Self::Rollback => "Rollback",
            Self::SetReadOnly { .. } => "SetReadOnly",
            Self::SetStrict => "SetStrict",
            Self::Close => "Close",
        };
        f.write_str(name)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum FromDriver {
    /// A result item of the running `Execute`, `Query` or `Stream`, in order.
    Item { id: u64, item: ExecItem },
    Reply {
        id: u64,
        result: Result<Reply, WireError>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Reply {
    Done,
    Connected { server_version: String },
    Affected(u64),
    Names(Vec<String>),
    Objects(Vec<SchemaObject>),
    Columns(Vec<TableColumns>),
    Table(TableInfo),
    ServerInfo(Vec<ServerInfoRow>),
    Statements(Option<Result<Vec<String>, String>>),
}

/// [`DriverError`] as data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WireError {
    Connect(String),
    Auth(String),
    Query {
        message: String,
        location: Option<ErrorLocation>,
    },
    ConnectionLost(String),
    Cancelled,
    Other(String),
}

impl From<DriverError> for WireError {
    fn from(err: DriverError) -> Self {
        match err {
            DriverError::Connect(message) => Self::Connect(message),
            DriverError::Auth(message) => Self::Auth(message),
            DriverError::Query { message, location } => Self::Query { message, location },
            DriverError::ConnectionLost(message) => Self::ConnectionLost(message),
            DriverError::Cancelled => Self::Cancelled,
            other @ (DriverError::Unsupported(_) | DriverError::Other(_)) => {
                Self::Other(other.to_string())
            }
        }
    }
}

impl From<WireError> for DriverError {
    fn from(err: WireError) -> Self {
        match err {
            WireError::Connect(message) => Self::Connect(message),
            WireError::Auth(message) => Self::Auth(message),
            WireError::Query { message, location } => Self::Query { message, location },
            WireError::ConnectionLost(message) => Self::ConnectionLost(message),
            WireError::Cancelled => Self::Cancelled,
            WireError::Other(message) => Self::Other(message.into()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("malformed message: {0}")]
    Json(#[from] serde_json::Error),
    #[error("message of {0} bytes is too large")]
    TooLarge(u64),
    #[error("unexpected frame kind {0}")]
    Kind(u8),
}

const MESSAGE: u8 = 0;
const ROWS: u8 = 1;

/// One frame: a JSON message, or a batch of rows of call `id`, still encoded.
#[derive(Debug)]
pub enum Frame<T> {
    Message(T),
    Rows { id: u64, batch: RowBatch },
}

pub async fn write_frame<W, T>(writer: &mut W, message: &T) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let mut frame = vec![0, 0, 0, 0, MESSAGE];
    serde_json::to_writer(&mut frame, message)?;
    send(writer, frame).await
}

pub async fn write_rows<W>(writer: &mut W, id: u64, batch: &RowBatch) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
{
    let mut frame = Vec::with_capacity(13 + batch.as_bytes().len());
    frame.extend_from_slice(&[0, 0, 0, 0, ROWS]);
    frame.extend_from_slice(&id.to_le_bytes());
    frame.extend_from_slice(batch.as_bytes());
    send(writer, frame).await
}

/// Sends a frame whose first 4 bytes are left for its length, in one write.
async fn send<W: AsyncWrite + Unpin>(writer: &mut W, mut frame: Vec<u8>) -> Result<(), FrameError> {
    let body = frame.len() - 4;
    let len = u32::try_from(body)
        .ok()
        .filter(|len| *len <= MAX_FRAME)
        .ok_or(FrameError::TooLarge(body as u64))?;
    frame[..4].copy_from_slice(&len.to_le_bytes());
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(())
}

/// The next frame, or `None` when the other side closed the stream between frames.
pub async fn read_frame<R, T>(reader: &mut R) -> Result<Option<Frame<T>>, FrameError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut len = [0u8; 4];
    match reader.read_exact(&mut len).await {
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err.into()),
    }
    let len = u32::from_le_bytes(len);
    if len > MAX_FRAME {
        return Err(FrameError::TooLarge(len.into()));
    }
    let mut body = vec![0u8; len as usize];
    reader.read_exact(&mut body).await?;
    match body.split_first() {
        Some((&MESSAGE, json)) => Ok(Some(Frame::Message(serde_json::from_slice(json)?))),
        Some((&ROWS, rest)) if rest.len() >= 8 => {
            let (id, rows) = rest.split_at(8);
            let mut id_bytes = [0u8; 8];
            id_bytes.copy_from_slice(id);
            // Left encoded: the engine writes the UI's JSON straight from the bytes.
            Ok(Some(Frame::Rows {
                id: u64::from_le_bytes(id_bytes),
                batch: RowBatch::from_bytes(rows.to_vec()),
            }))
        }
        Some((&kind, _)) => Err(FrameError::Kind(kind)),
        None => Err(FrameError::Kind(u8::MAX)),
    }
}

/// The next frame, which must be a JSON message.
pub async fn read_message<R, T>(reader: &mut R) -> Result<Option<T>, FrameError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    match read_frame(reader).await? {
        None => Ok(None),
        Some(Frame::Message(message)) => Ok(Some(message)),
        Some(Frame::Rows { .. }) => Err(FrameError::Kind(ROWS)),
    }
}

#[cfg(test)]
mod tests {
    use quarry_model::{ConnId, Environment, SslMode};

    use super::*;

    #[tokio::test]
    async fn frames_round_trip_exact_values() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let rows = vec![vec![
            Value::Float(0.1 + 0.2),
            Value::UInt(u64::MAX),
            Value::Int(i64::MIN),
            Value::Bytes(vec![0, 255]),
        ]];
        let sent = FromDriver::Item {
            id: 3,
            item: ExecItem::Rows(rows.clone()),
        };
        write_frame(&mut a, &sent).await.unwrap();
        drop(a);
        let got: FromDriver = read_message(&mut b).await.unwrap().unwrap();
        let FromDriver::Item {
            id: 3,
            item: ExecItem::Rows(got),
        } = got
        else {
            panic!("unexpected {got:?}");
        };
        assert_eq!(got, rows);
        assert!(
            read_message::<_, FromDriver>(&mut b)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_huge_length_is_refused_before_reading() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&u32::MAX.to_le_bytes()).await.unwrap();
        let err = read_message::<_, FromDriver>(&mut b).await.unwrap_err();
        assert!(matches!(err, FrameError::TooLarge(_)));
    }

    #[tokio::test]
    async fn rows_travel_in_their_own_frames() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let rows = vec![vec![Value::Int(7), Value::Text("x".into())]];
        write_rows(&mut a, 9, &RowBatch::encode(&rows))
            .await
            .unwrap();
        let frame = read_frame::<_, FromDriver>(&mut b).await.unwrap().unwrap();
        let Frame::Rows { id: 9, batch } = frame else {
            panic!("rows expected");
        };
        assert_eq!(batch.decode().unwrap(), rows);
        write_rows(&mut a, 9, &batch).await.unwrap();
        assert!(read_message::<_, FromDriver>(&mut b).await.is_err());
    }

    #[test]
    fn debug_hides_the_password() {
        let call = Call::Connect {
            config: ConnectionConfig {
                id: ConnId::new(),
                name: "x".into(),
                driver: DriverId("mysql".into()),
                host: None,
                port: None,
                database: None,
                user: None,
                path: None,
                ssl: SslMode::Disable,
                ssh: None,
                environment: Environment::Local,
                read_only: false,
                color: None,
                group: None,
            },
            password: Some("hunter2".into()),
        };
        let text = format!("{call:?}");
        assert!(!text.contains("hunter2"));
        assert!(text.contains("***"));
    }
}
