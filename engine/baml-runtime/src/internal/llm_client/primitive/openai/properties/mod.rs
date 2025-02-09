pub(crate) mod azure;
pub(crate) mod generic;
pub(crate) mod ollama;
pub(crate) mod openai;

use crate::internal::llm_client::{AllowedMetadata, SupportedRequestModes};
use std::collections::HashMap;

pub struct PostRequestProperties {
    pub default_role: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub headers: HashMap<String, String>,
    pub query_params: HashMap<String, String>,
    pub proxy_url: Option<String>,
    // These are passed directly to the OpenAI API.
    pub properties: HashMap<String, serde_json::Value>,
    pub allowed_metadata: AllowedMetadata,
    pub supported_request_modes: SupportedRequestModes,
}
