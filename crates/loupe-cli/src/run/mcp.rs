use loupe_core::backend::Backend;
use loupe_core::config::Profile;
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::mcp::{Classify, Engine, Select};
use loupe_core::mcp_access::Settings;
use loupe_core::preset_store::Store;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, ReadBuf};

struct Input<R> {
    reader: R,
    bytes: usize,
    limit: usize,
}
impl<R: AsyncRead + Unpin> AsyncRead for Input<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let input = self.get_mut();
        let before = buffer.filled().len();
        match Pin::new(&mut input.reader).poll_read(context, buffer) {
            Poll::Ready(Ok(())) => {
                for byte in &buffer.filled()[before..] {
                    input.bytes = if *byte == b'\n' {
                        0
                    } else {
                        input.bytes.saturating_add(1)
                    };
                    if input.bytes > input.limit {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "MCP message exceeds the configured input bound",
                        )));
                    }
                }
                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

struct Server {
    engine: Engine,
    backend: Backend,
    tools: Vec<Tool>,
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("loupe", env!("CARGO_PKG_VERSION")))
            .with_instructions("Read-only classification and source-backed selection. Use state or an allowlisted path. Presets and paths require explicit administrator allowlists. Responses retain raw model answers; token plans perform no inference.")
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        if request.is_some_and(|request| request.cursor.is_some()) {
            return Err(ErrorData::invalid_params(
                "Loupe's tool list has no pagination cursor",
                None,
            ));
        }
        Ok(ListToolsResult::with_all_items(self.tools.clone()))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|tool| tool.name == name).cloned()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        let operation = async {
            match request.name.as_ref() {
                "presets" => {
                    #[derive(Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Empty {}
                    let _: Empty = parse(arguments)?;
                    Ok(self.engine.presets())
                }
                "tokens" => self.engine.plan(parse::<Classify>(arguments)?),
                "classify" | "ask" => {
                    self.engine
                        .classify(
                            &self.backend,
                            parse::<Classify>(arguments)?,
                            request.name == "ask",
                        )
                        .await
                }
                "find" | "pick" | "extract" => {
                    let mut parameters: Select = parse(arguments)?;
                    if request.name == "find" && parameters.take.is_none() {
                        parameters.take = Some(1);
                    }
                    self.engine
                        .select(&self.backend, parameters, request.name == "extract")
                        .await
                }
                _ => Err(Error::new(ErrorKind::Usage, "unknown Loupe tool")),
            }
        };
        let result = tokio::select! {
            biased;
            _ = context.ct.cancelled() => Err(Error::new(ErrorKind::Unavailable, "MCP tool call cancelled")),
            result = operation => result,
        };
        Ok(match result {
            Ok(value) => CallToolResult::structured(value),
            Err(error) => CallToolResult::structured_error(
                json!({"error":error.to_string(),"exit_code":error.exit_code()}),
            ),
        }
        .into())
    }
}

fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value)
        .map_err(|error| Error::new(ErrorKind::Usage, format!("invalid tool arguments: {error}")))
}

fn tools() -> Result<Vec<Tool>> {
    let question = json!({"oneOf":[
        {"type":"object","additionalProperties":false,"required":["type"],"properties":{"type":{"const":"noul"},"instructions":{},"criteria":{"type":"object","additionalProperties":false,"properties":{"true":{},"false":{}}}}},
        {"type":"object","additionalProperties":false,"required":["type","criteria"],"properties":{"type":{"const":"choice"},"instructions":{},"criteria":{"type":"object","minProperties":1,"propertyNames":{"minLength":1},"additionalProperties":{}}}},
        {"type":"object","additionalProperties":false,"required":["type","criteria"],"properties":{"type":{"const":"score"},"instructions":{},"criteria":{"type":"array","minItems":1,"items":{}}}}
    ]});
    let mut classifier = json!({
        "type":"object","additionalProperties":false,
        "properties":{
            "state":{},"path":{"type":"string","minLength":1},
            "question":{"type":"object"},"questions":{"type":"object","minProperties":1},
            "preset":{"type":"string","minLength":1},"args":{"type":"object","additionalProperties":{"type":"string"}},
            "ensemble":{"type":"boolean"},"min_agreement":{"type":"number","minimum":0,"maximum":1}
        },
        "allOf":[{"oneOf":[{"required":["state"]},{"required":["path"]}]},{"oneOf":[{"required":["question"]},{"required":["questions"]},{"required":["preset"]}]}]
    });
    classifier["properties"]["question"] = question.clone();
    classifier["properties"]["questions"]["additionalProperties"] = question;
    classifier["properties"]["questions"]["propertyNames"] = json!({"minLength":1});
    let selection = json!({
        "type":"object","additionalProperties":false,
        "properties":{"state":{"type":"string"},"path":{"type":"string","minLength":1},"query":{"type":"string","minLength":1},"take":{"type":"integer","minimum":1}},
        "required":["query"],"oneOf":[{"required":["state"]},{"required":["path"]}]
    });
    let mut extraction = selection.clone();
    extraction["properties"]["candidates"] =
        json!({"type":"string","enum":["dates","money","semver","emails","hostnames","tickers"]});
    extraction["required"] = json!(["query", "candidates"]);
    extraction["properties"]["normalize"] = json!({"type":"object","additionalProperties":false,"properties":{"now":{"type":"string"},"tz":{"type":"string"},"currency":{"type":"string"},"decimal_separator":{"enum":["dot","comma"]},"date_order":{"enum":["mdy","dmy"]}}});
    [
        ("classify", "Evaluate typed Noul/Choice/Score questions or an allowlisted preset; return raw answers and shared policy decisions.", classifier.clone()),
        ("ask", "Return the exact System One API response for all supplied questions.", classifier.clone()),
        ("tokens", "Count and inspect a complete request without inference, credential execution or cache writes.", classifier),
        ("find", "Select original source lines answering a query, with presence and none gates; take defaults to one.", selection.clone()),
        ("pick", "Rank up to take original lines using Choice marginals and bounded tournaments.", selection),
        ("extract", "Select source-backed parsed spans and byte offsets, with optional deterministic normalization.", extraction),
        ("presets", "List only explicitly allowlisted trusted presets.", json!({"type":"object","additionalProperties":false})),
    ].into_iter().map(|(name, description, schema)| {
        let schema = schema.as_object().cloned().ok_or_else(|| Error::new(ErrorKind::Config, "MCP tool schema must be an object"))?;
        let local = matches!(name, "tokens" | "presets");
        let annotations = ToolAnnotations::from_raw(None, Some(true), Some(false), Some(local), Some(!local));
        Ok(Tool::new(name, description, schema).with_annotations(annotations))
    }).collect()
}

pub async fn execute(
    store: Store,
    profile: Profile,
    settings: &Settings,
    deadline: Duration,
    budget: Option<u64>,
) -> Result<u8> {
    let engine = Engine::new(settings, store, profile, deadline, budget)?;
    // Credentials are resolved once from trusted configuration, before serving.
    // Tool arguments cannot launch commands or change the endpoint/profile.
    let backend = loupe_core::backend::with_deadline(
        Some(
            tokio::time::Instant::now()
                .checked_add(deadline)
                .ok_or_else(|| Error::new(ErrorKind::Usage, "MCP deadline outside clock range"))?,
        ),
        Backend::new(engine.profile()),
    )
    .await?;
    let server = Server {
        engine,
        backend,
        tools: tools()?,
    };
    let stdin = loupe_core::stdio::Stream::new(std::io::stdin())
        .map_err(|error| Error::io("opening MCP stdin", error))?;
    let stdout = loupe_core::stdio::Stream::new(std::io::stdout())
        .map_err(|error| Error::io("opening MCP stdout", error))?;
    let limit = settings
        .byte_limit()
        .checked_add(65536)
        .ok_or_else(|| Error::new(ErrorKind::Usage, "MCP message bound outside reader range"))?;
    let input = Input {
        reader: stdin,
        bytes: 0,
        limit,
    };
    let startup = server.serve((input, stdout));
    let interrupt = crate::run::interrupt_signal();
    tokio::pin!(startup, interrupt);
    let service = tokio::select! {
        result = &mut startup => result.map_err(|error| Error::new(ErrorKind::Unavailable, format!("starting MCP stdio: {error}")))?,
        code = &mut interrupt => return code,
    };
    let cancel = service.cancellation_token();
    let waiting = service.waiting();
    tokio::pin!(waiting);
    tokio::select! {
        result = &mut waiting => { result.map_err(|error| Error::new(ErrorKind::Unavailable, format!("MCP stdio task: {error}")))?; Ok(0) },
        code = &mut interrupt => {
            cancel.cancel();
            waiting.await.map_err(|error| Error::new(ErrorKind::Unavailable, format!("stopping MCP stdio: {error}")))?;
            code
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[allow(clippy::unwrap_used)]
    fn compatibility_mcp_tool_catalog() {
        let catalog =
            rmcp::model::ListToolsResult::with_all_items(crate::run::mcp::tools().unwrap());
        crate::compatibility::snapshot_json(
            "mcp-tools.json",
            &serde_json::to_value(catalog).unwrap(),
        );
    }
}
