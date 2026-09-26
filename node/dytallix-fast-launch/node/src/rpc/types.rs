use serde::Serialize;

#[derive(Serialize)]
pub struct ErrorResponse { pub error: String, pub message: String }

#[derive(Serialize)]
pub struct SubmitResponse { pub hash: String, pub status: String }
