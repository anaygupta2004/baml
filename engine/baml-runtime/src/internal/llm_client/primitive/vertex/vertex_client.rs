use crate::client_registry::ClientProperty;
use crate::internal::llm_client::properties_hander::{ PropertiesHandler};
use crate::internal::llm_client::traits::{
    ToProviderMessage, ToProviderMessageExt, WithClientProperties,
};
use crate::internal::llm_client::{AllowedMetadata, ResolveMediaUrls, SupportedRequestModes};
use crate::RuntimeContext;
use crate::{
    internal::llm_client::{
        primitive::{
            request::{make_parsed_request, make_request, RequestBuilder},
            vertex::types::{FinishReason, VertexResponse},
        },
        traits::{
            SseResponseTrait, StreamResponse, WithChat, WithClient, WithNoCompletion,
            WithRetryPolicy, WithStreamChat,
        },
        ErrorCode, LLMCompleteResponse, LLMCompleteResponseMetadata, LLMErrorResponse, LLMResponse,
        ModelFeatures,
    },
    request::create_client,
};
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use futures::StreamExt;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(not(target_arch = "wasm32"))]
use std::fs::File;
#[cfg(not(target_arch = "wasm32"))]
use std::io::BufReader;

use baml_types::BamlMediaContent;
use eventsource_stream::Eventsource;
use internal_baml_core::ir::ClientWalker;
use internal_baml_jinja::{RenderContext_Client, RenderedChatMessage};

use serde_json::json;
use std::collections::HashMap;

enum ServiceAccountDetails {
    None,
    RawAuthorizationHeader(String),
    FilePath(String),
    Json(serde_json::Map<String, serde_json::Value>),
}

impl Default for ServiceAccountDetails {
    fn default() -> Self {
        ServiceAccountDetails::None
    }
}

struct PostRequestProperties {
    default_role: String,
    base_url: Option<String>,
    service_account_details: ServiceAccountDetails,
    headers: HashMap<String, String>,
    proxy_url: Option<String>,
    properties: HashMap<String, serde_json::Value>,
    project_id: Option<String>,
    model_id: Option<String>,
    location: Option<String>,
    allowed_metadata: AllowedMetadata,
    supported_request_modes: SupportedRequestModes,
}

pub struct VertexClient {
    pub name: String,
    pub client: reqwest::Client,
    pub retry_policy: Option<String>,
    pub context: RenderContext_Client,
    pub features: ModelFeatures,
    properties: PostRequestProperties,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iss: String,
    scope: String,
    aud: String,
    exp: i64,
    iat: i64,
}

#[derive(Debug, Deserialize)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
}

fn resolve_properties(
    mut properties: PropertiesHandler,
    ctx: &RuntimeContext,
) -> Result<PostRequestProperties, anyhow::Error> {
    let default_role = properties.pull_default_role("user")?;

    let base_url = properties.pull_base_url()?;
    let allowed_metadata = properties.pull_allowed_role_metadata()?;

    let service_account_details = {
        let authz = properties.remove_str("authorization")?;
        let creds = properties.remove_serde::<serde_json::Value>("credentials")?;
        let creds_content = properties.remove_str("credentials_content")?;

        // Ensure that at most one of authz, creds, and creds_content is provided
        if [authz.is_some(), creds.is_some(), creds_content.is_some()]
            .iter()
            .filter(|&&b| b)
            .count()
            > 1
        {
            anyhow::bail!(
                "Only one of authorization, credentials, and credentials_content can be provided"
            );
        }

        if let Some(authz) = authz {
            ServiceAccountDetails::RawAuthorizationHeader(authz)
        } else if let Some(creds) = creds {
            match creds {
                serde_json::Value::String(s) => match serde_json::from_str(&s) {
                    Ok(service_account) => ServiceAccountDetails::Json(service_account),
                    Err(_) => ServiceAccountDetails::FilePath(s),
                },
                serde_json::Value::Object(o) => ServiceAccountDetails::Json(o),
                _ => anyhow::bail!("credentials must be a string or JSON object"),
            }
        } else if let Some(creds_content) = creds_content {
            ServiceAccountDetails::Json(
                serde_json::from_str(&creds_content)
                    .context("Failed to parse credentials_content as a JSON object")?,
            )
        } else if let Some(path) = ctx.env.get("GOOGLE_APPLICATION_CREDENTIALS") {
            ServiceAccountDetails::FilePath(path.to_string())
        } else if let Some(creds_content) = ctx.env.get("GOOGLE_APPLICATION_CREDENTIALS_CONTENT") {
            ServiceAccountDetails::Json(
                serde_json::from_str(&creds_content)
                    .context("Failed to parse credentials_content as a JSON object")?,
            )
        } else {
            ServiceAccountDetails::None
        }
    };
    let headers = properties.pull_headers()?;

    let project_id = properties.remove_str("project_id")?;
    let model_id = properties.remove_str("model")?;
    let location = properties.remove_str("location")?;

    // Ensure that project_id, model_id, and location are provided
    let project_id = match project_id {
        Some(project_id) => project_id,
        None => anyhow::bail!("project_id must be provided"),
    };
    let model_id = match model_id {
        Some(model_id) => model_id,
        None => anyhow::bail!("model must be provided"),
    };
    let location = match location {
        Some(location) => location,
        None => anyhow::bail!("location must be provided"),
    };

    let supported_request_modes = properties.pull_supported_request_modes()?;

    Ok(PostRequestProperties {
        default_role,
        base_url,
        service_account_details,
        headers,
        properties: properties.finalize(),
        project_id: Some(project_id),
        model_id: Some(model_id),
        location: Some(location),
        proxy_url: ctx.env.get("BOUNDARY_PROXY_URL").map(|s| s.to_string()),
        allowed_metadata,
        supported_request_modes,
    })
}

impl WithRetryPolicy for VertexClient {
    fn retry_policy_name(&self) -> Option<&str> {
        self.retry_policy.as_deref()
    }
}

impl WithClientProperties for VertexClient {
    fn client_properties(&self) -> &HashMap<String, serde_json::Value> {
        &self.properties.properties
    }
    fn allowed_metadata(&self) -> &crate::internal::llm_client::AllowedMetadata {
        &self.properties.allowed_metadata
    }
    fn supports_streaming(&self) -> bool {
        self.properties.supported_request_modes.stream.unwrap_or(true)
    }
}

impl WithClient for VertexClient {
    fn context(&self) -> &RenderContext_Client {
        &self.context
    }

    fn model_features(&self) -> &ModelFeatures {
        &self.features
    }
}

impl WithNoCompletion for VertexClient {}

impl SseResponseTrait for VertexClient {
    fn response_stream(
        &self,
        resp: reqwest::Response,
        prompt: &Vec<RenderedChatMessage>,
        system_start: web_time::SystemTime,
        instant_start: web_time::Instant,
    ) -> StreamResponse {
        let prompt = prompt.clone();
        let client_name = self.context.name.clone();
        let model_id = self.properties.model_id.clone().unwrap_or_default();
        let params = self.properties.properties.clone();
        Ok(Box::pin(
            resp.bytes_stream()
                .eventsource()
                .inspect(|event| log::trace!("Received event: {:?}", event))
                .take_while(|event| {
                    std::future::ready(event.as_ref().is_ok_and(|e| e.data != "data: \n"))
                })
                .map(|event| -> Result<VertexResponse> {
                    Ok(serde_json::from_str::<VertexResponse>(&event?.data)?)
                })
                .scan(
                    Ok(LLMCompleteResponse {
                        client: client_name.clone(),
                        prompt: internal_baml_jinja::RenderedPrompt::Chat(prompt.clone()),
                        content: "".to_string(),
                        start_time: system_start,
                        latency: instant_start.elapsed(),
                        model: model_id,
                        request_options: params.clone(),
                        metadata: LLMCompleteResponseMetadata {
                            baml_is_complete: false,
                            finish_reason: None,
                            prompt_tokens: None,
                            output_tokens: None,
                            total_tokens: None,
                        },
                    }),
                    move |accumulated: &mut Result<LLMCompleteResponse>, event| {
                        let Ok(ref mut inner) = accumulated else {
                            // halt the stream: the last stream event failed to parse
                            return std::future::ready(None);
                        };
                        let event = match event {
                            Ok(event) => event,
                            Err(e) => {
                                return std::future::ready(Some(LLMResponse::LLMFailure(
                                    LLMErrorResponse {
                                        client: client_name.clone(),
                                        model: if inner.model == "" {
                                            None
                                        } else {
                                            Some(inner.model.clone())
                                        },
                                        prompt: internal_baml_jinja::RenderedPrompt::Chat(
                                            prompt.clone(),
                                        ),
                                        start_time: system_start,
                                        request_options: params.clone(),
                                        latency: instant_start.elapsed(),
                                        message: format!("Failed to parse event: {:#?}", e),
                                        code: ErrorCode::UnsupportedResponse(2),
                                    },
                                )));
                            }
                        };

                        if let Some(choice) = event.candidates.get(0) {
                            if let Some(content) = choice.content.parts.get(0) {
                                inner.content += &content.text;
                            }
                            match choice.finish_reason.as_ref() {
                                Some(FinishReason::Stop) => {
                                    inner.metadata.baml_is_complete = true;
                                    inner.metadata.finish_reason =
                                        Some(FinishReason::Stop.to_string());
                                }
                                _ => (),
                            }
                        }
                        inner.latency = instant_start.elapsed();

                        std::future::ready(Some(LLMResponse::Success(inner.clone())))
                    },
                ),
        ))
    }
}
// makes the request to the google client, on success it triggers the response_stream function to handle continuous rendering with the response object
impl WithStreamChat for VertexClient {
    async fn stream_chat(
        &self,
        ctx: &RuntimeContext,
        prompt: &Vec<RenderedChatMessage>,
    ) -> StreamResponse {
        //incomplete, streaming response object is returned
        let (response, system_now, instant_now) =
            match make_request(self, either::Either::Right(prompt), true).await {
                Ok(v) => v,
                Err(e) => return Err(e),
            };
        self.response_stream(response, prompt, system_now, instant_now)
    }
}

impl VertexClient {
    pub fn new(client: &ClientWalker, ctx: &RuntimeContext) -> Result<Self> {
        let properties = super::super::resolve_properties_walker(client, ctx)?;
        let properties = resolve_properties(properties, ctx)?;
        let default_role = properties.default_role.clone();
        Ok(Self {
            name: client.name().into(),
            context: RenderContext_Client {
                name: client.name().into(),
                provider: client.elem().provider.clone(),
                default_role,
            },
            features: ModelFeatures {
                chat: true,
                completion: false,
                anthropic_system_constraints: false,
                resolve_media_urls: ResolveMediaUrls::EnsureMime,
                allowed_metadata: properties.allowed_metadata.clone(),
            },
            retry_policy: client
                .elem()
                .retry_policy_id
                .as_ref()
                .map(|s| s.to_string()),
            client: create_client()?,
            properties,
        })
    }

    pub fn dynamic_new(client: &ClientProperty, ctx: &RuntimeContext) -> Result<Self> {
        let properties = resolve_properties(client.property_handler()?, ctx)?;
        let default_role = properties.default_role.clone();

        Ok(Self {
            name: client.name.clone(),
            context: RenderContext_Client {
                name: client.name.clone(),
                provider: client.provider.clone(),
                default_role,
            },
            features: ModelFeatures {
                chat: true,
                completion: false,
                anthropic_system_constraints: false,
                resolve_media_urls: ResolveMediaUrls::EnsureMime,
                allowed_metadata: properties.allowed_metadata.clone(),
            },
            retry_policy: client.retry_policy.clone(),
            client: create_client()?,
            properties,
        })
    }
}

async fn get_access_token(service_account: &ServiceAccount) -> Result<String> {
    let now = Utc::now();
    let claims = Claims {
        iss: service_account.client_email.clone(),
        scope: "https://www.googleapis.com/auth/cloud-platform".to_string(),
        aud: "https://oauth2.googleapis.com/token".to_string(),
        exp: (now + Duration::hours(1)).timestamp(),
        iat: now.timestamp(),
    };

    // Create the JWT
    let header = Header::new(Algorithm::RS256);
    let key = EncodingKey::from_rsa_pem(service_account.private_key.as_bytes())?;
    let jwt = encode(&header, &claims, &key)?;

    // Make the token request
    let client = reqwest::Client::new();
    let params = [
        ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
        ("assertion", &jwt),
    ];
    let res: Value = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await?
        .json()
        .await?;

    Ok(res
        .as_object()
        .context("Token exchange did not return a JSON object")?
        .get("access_token")
        .context("Access token not found in response")?
        .as_str()
        .context("Access token is not a string")?
        .to_string())
}

impl RequestBuilder for VertexClient {
    fn http_client(&self) -> &reqwest::Client {
        &self.client
    }

    async fn build_request(
        &self,
        prompt: either::Either<&String, &Vec<RenderedChatMessage>>,
        allow_proxy: bool,
        stream: bool,
    ) -> Result<reqwest::RequestBuilder> {
        //disabled proxying for testing

        let mut should_stream = "generateContent";
        if stream {
            should_stream = "streamGenerateContent?alt=sse";
        }

        let location = self
            .properties
            .location
            .clone()
            .unwrap_or_else(|| "".to_string());
        let project_id = self
            .properties
            .project_id
            .clone()
            .unwrap_or_else(|| "".to_string());

        let model_id = self
            .properties
            .model_id
            .clone()
            .unwrap_or_else(|| "".to_string());

        let base_url = self
            .properties
            .base_url
            .clone()
            .unwrap_or_else(|| "".to_string());

        let baml_original_url = if base_url != "" {
            format!("{}{}:{}", base_url, model_id, should_stream)
        } else {
            format!(
                "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/publishers/google/models/{}:{}",
                location,
                project_id,
                location,
                model_id,
                should_stream
            )
        };
        let mut req = match (&self.properties.proxy_url, allow_proxy) {
            (Some(proxy_url), true) => {
                let req = self.client.post(proxy_url.clone());
                req.header("baml-original-url", baml_original_url)
            }
            _ => self.client.post(baml_original_url),
        };

        let access_token = match &self.properties.service_account_details {
            ServiceAccountDetails::None => {
                anyhow::bail!("No service account was specified.");
            }
            ServiceAccountDetails::RawAuthorizationHeader(token) => token.to_string(),
            ServiceAccountDetails::FilePath(path) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let file = File::open(path)?;
                    let reader = BufReader::new(file);
                    let service_account: ServiceAccount = serde_json::from_reader(reader)?;

                    get_access_token(&service_account).await?
                }
                #[cfg(target_arch = "wasm32")]
                {
                    anyhow::bail!(
                        "Reading from files not supported in BAML playground. Pass in your credentials file as a string to the 'GOOGLE_APPLICATION_CREDENTIALS_CONTENT' environment variable."
                    );
                }
            }
            ServiceAccountDetails::Json(token) => {
                let service_account: ServiceAccount =
                    serde_json::from_value(serde_json::Value::Object(token.clone()))?;
                get_access_token(&service_account).await?
            }
        };

        req = req.header("Authorization", format!("Bearer {}", access_token));

        for (key, value) in &self.properties.headers {
            req = req.header(key, value);
        }

        let mut body = json!(self.properties.properties);
        let body_obj = body.as_object_mut().unwrap();

        match prompt {
            either::Either::Left(prompt) => {
                body_obj.extend(convert_completion_prompt_to_body(prompt))
            }
            either::Either::Right(messages) => body_obj.extend(self.chat_to_message(messages)?),
        }

        Ok(req.json(&body))
    }
    fn request_options(&self) -> &HashMap<String, serde_json::Value> {
        &self.properties.properties
    }
}

impl WithChat for VertexClient {
    fn chat_options(&self, _ctx: &RuntimeContext) -> Result<internal_baml_jinja::ChatOptions> {
        Ok(internal_baml_jinja::ChatOptions::new(
            self.properties.default_role.clone(),
            None,
        ))
    }

    async fn chat(&self, _ctx: &RuntimeContext, prompt: &Vec<RenderedChatMessage>) -> LLMResponse {
        //non-streaming, complete response is returned
        let (response, system_now, instant_now) =
            match make_parsed_request::<VertexResponse>(self, either::Either::Right(prompt), false)
                .await
            {
                Ok(v) => v,
                Err(e) => return e,
            };

        if response.candidates.len() != 1 {
            return LLMResponse::LLMFailure(LLMErrorResponse {
                client: self.context.name.to_string(),
                model: None,
                prompt: internal_baml_jinja::RenderedPrompt::Chat(prompt.clone()),
                start_time: system_now,
                request_options: self.properties.properties.clone(),
                latency: instant_now.elapsed(),
                message: format!(
                    "Expected exactly one content block, got {}",
                    response.candidates.len()
                ),
                code: ErrorCode::Other(200),
            });
        }
        let usage_metadata = response.usage_metadata.clone().unwrap();

        LLMResponse::Success(LLMCompleteResponse {
            client: self.context.name.to_string(),
            prompt: internal_baml_jinja::RenderedPrompt::Chat(prompt.clone()),
            content: response.candidates[0].content.parts[0].text.clone(),
            start_time: system_now,
            latency: instant_now.elapsed(),
            request_options: self.properties.properties.clone(),
            model: self
                .properties
                .properties
                .get("model")
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "".to_string()),
            metadata: LLMCompleteResponseMetadata {
                baml_is_complete: match response.candidates[0].finish_reason {
                    Some(FinishReason::Stop) => true,
                    _ => false,
                },
                finish_reason: response.candidates[0]
                    .finish_reason
                    .as_ref()
                    .map(|r| serde_json::to_string(r).unwrap_or("".into())),
                prompt_tokens: usage_metadata.prompt_token_count,
                output_tokens: usage_metadata.candidates_token_count,
                total_tokens: usage_metadata.total_token_count,
            },
        })
    }
}

//simple, Map with key "prompt" and value of the prompt string
fn convert_completion_prompt_to_body(prompt: &String) -> HashMap<String, serde_json::Value> {
    let mut map = HashMap::new();
    let content = json!({
        "role": "user",
        "parts": [{
            "text": prompt
        }]
    });
    map.insert("contents".into(), json!([content]));
    map
}

impl ToProviderMessage for VertexClient {
    fn to_chat_message(
        &self,
        mut content: serde_json::Map<String, serde_json::Value>,
        text: &str,
    ) -> Result<serde_json::Map<String, serde_json::Value>> {
        content.insert("text".into(), json!(text));
        Ok(content)
    }

    fn to_media_message(
        &self,
        mut content: serde_json::Map<String, serde_json::Value>,
        media: &baml_types::BamlMedia,
    ) -> Result<serde_json::Map<String, serde_json::Value>> {
        match &media.content {
            BamlMediaContent::File(_) => anyhow::bail!(
                "BAML internal error (Vertex): file should have been resolved to base64"
            ),
            BamlMediaContent::Url(data) => {
                content.insert(
                    "fileData".into(),
                    json!({"file_uri": data.url, "mime_type": media.mime_type}),
                );
                Ok(content)
            }
            BamlMediaContent::Base64(data) => {
                content.insert(
                    "inlineData".into(),
                    json!({
                        "data": data.base64,
                        "mime_type": media.mime_type_as_ok()?
                    }),
                );
                Ok(content)
            }
        }
    }

    fn role_to_message(
        &self,
        content: &RenderedChatMessage,
    ) -> Result<serde_json::Map<String, serde_json::Value>> {
        let mut map = serde_json::Map::new();
        map.insert("role".into(), json!(content.role));
        map.insert(
            "parts".into(),
            json!(self.parts_to_message(&content.parts)?),
        );
        Ok(map)
    }
}

impl ToProviderMessageExt for VertexClient {
    fn chat_to_message(
        &self,
        chat: &Vec<RenderedChatMessage>,
    ) -> Result<serde_json::Map<String, serde_json::Value>> {
        // merge all adjacent roles of the same type
        let mut res = serde_json::Map::new();

        res.insert(
            "contents".into(),
            chat.iter()
                .map(|c| self.role_to_message(c))
                .collect::<Result<Vec<_>>>()?
                .into(),
        );

        Ok(res)
    }
}
