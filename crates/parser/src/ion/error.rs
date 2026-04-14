use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IonError {
    Msg(String),
    BadDtype { dtype: u8, kind: &'static str },
}

pub type IonResult<T> = Result<T, IonError>;

impl IonError {
    pub fn contains(&self, text: &str) -> bool {
        self.to_string().contains(text)
    }
}

impl Display for IonError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Msg(text) => f.write_str(text),
            Self::BadDtype { dtype, kind } => {
                write!(f, "unsupported dtype {dtype} for {kind}")
            }
        }
    }
}

impl Error for IonError {}

impl From<String> for IonError {
    fn from(text: String) -> Self {
        Self::Msg(text)
    }
}

impl From<&str> for IonError {
    fn from(text: &str) -> Self {
        Self::Msg(text.to_owned())
    }
}

impl From<std::io::Error> for IonError {
    fn from(err: std::io::Error) -> Self {
        Self::Msg(err.to_string())
    }
}
