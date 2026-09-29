//! The request and response inside the channel: one of each per connection,
//! in the node HTTP adapter's two request forms (JSON-RPC `POST /` and URI
//! `GET /method`) and with its bounds.

use crate::Error;

pub const MAX_PATH: usize = 128;
pub const MAX_QUERY: usize = 16384;
pub const MAX_REQUEST_BODY: usize = 1_048_576;
pub const MAX_RESPONSE_BODY: usize = 1_500_000;
/// `Content-Type` and `Cache-Control`, the only response headers carried.
pub const MAX_HEADER_VALUE: usize = 256;
pub const MAX_REQUEST_MESSAGE: usize = 2 + 2 + MAX_PATH + 2 + MAX_QUERY + 4 + MAX_REQUEST_BODY;
pub const MAX_RESPONSE_MESSAGE: usize = 1 + 2 + 2 * (2 + MAX_HEADER_VALUE) + 4 + MAX_RESPONSE_BODY;

const REQUEST: u8 = 1;
const RESPONSE: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// URI form: `GET /method?query`, no body.
    Get = 1,
    /// JSON-RPC form: `POST /` with a body and no query.
    Post = 2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    pub path: String,
    /// The raw query string, without the `?`.
    pub query: String,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    /// Empty when the node sent none.
    pub cache_control: String,
    pub body: Vec<u8>,
}

fn visible(value: &str) -> bool {
    value.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

fn header_value(value: &str) -> bool {
    value.len() <= MAX_HEADER_VALUE && value.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

impl Request {
    fn validate(&self) -> Result<(), Error> {
        let path = self.path.len() <= MAX_PATH && self.path.starts_with('/') && visible(&self.path);
        let query = self.query.len() <= MAX_QUERY && visible(&self.query);
        let form = match self.method {
            Method::Get => self.body.is_empty(),
            Method::Post => self.path == "/" && self.query.is_empty() && !self.body.is_empty(),
        };
        if path && query && form && self.body.len() <= MAX_REQUEST_BODY {
            Ok(())
        } else {
            Err(Error::Malformed)
        }
    }

    /// `[1][method][u16 path][u16 query][u32 body]`, lengths big-endian.
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        self.validate()?;
        let mut out = vec![REQUEST, self.method as u8];
        put(&mut out, self.path.as_bytes(), 2);
        put(&mut out, self.query.as_bytes(), 2);
        put(&mut out, &self.body, 4);
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut input = Input(bytes);
        if input.byte()? != REQUEST {
            return Err(Error::Malformed);
        }
        let method = match input.byte()? {
            1 => Method::Get,
            2 => Method::Post,
            _ => return Err(Error::Malformed),
        };
        let request = Request {
            method,
            path: input.text(2)?,
            query: input.text(2)?,
            body: input.field(4)?.to_vec(),
        };
        input.end()?;
        request.validate()?;
        Ok(request)
    }
}

impl Response {
    fn validate(&self) -> Result<(), Error> {
        if (100..=599).contains(&self.status)
            && header_value(&self.content_type)
            && header_value(&self.cache_control)
            && self.body.len() <= MAX_RESPONSE_BODY
        {
            Ok(())
        } else {
            Err(Error::Malformed)
        }
    }

    /// `[2][u16 status][u16 content type][u16 cache control][u32 body]`.
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        self.validate()?;
        let mut out = vec![RESPONSE];
        out.extend_from_slice(&self.status.to_be_bytes());
        put(&mut out, self.content_type.as_bytes(), 2);
        put(&mut out, self.cache_control.as_bytes(), 2);
        put(&mut out, &self.body, 4);
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut input = Input(bytes);
        if input.byte()? != RESPONSE {
            return Err(Error::Malformed);
        }
        let status = u16::from_be_bytes(input.take(2)?.try_into().expect("two bytes"));
        let response = Response {
            status,
            content_type: input.text(2)?,
            cache_control: input.text(2)?,
            body: input.field(4)?.to_vec(),
        };
        input.end()?;
        response.validate()?;
        Ok(response)
    }
}

fn put(out: &mut Vec<u8>, value: &[u8], width: usize) {
    let len = value.len() as u32;
    out.extend_from_slice(&len.to_be_bytes()[4 - width..]);
    out.extend_from_slice(value);
}

struct Input<'a>(&'a [u8]);

impl<'a> Input<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.0.len() < n {
            return Err(Error::Malformed);
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn byte(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn field(&mut self, width: usize) -> Result<&'a [u8], Error> {
        let mut len = [0u8; 4];
        len[4 - width..].copy_from_slice(self.take(width)?);
        self.take(u32::from_be_bytes(len) as usize)
    }
    fn text(&mut self, width: usize) -> Result<String, Error> {
        String::from_utf8(self.field(width)?.to_vec()).map_err(|_| Error::Malformed)
    }
    fn end(&self) -> Result<(), Error> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(Error::Malformed)
        }
    }
}
