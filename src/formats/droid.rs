use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::ir::{
    ContentBlock, MessageEvent, ReasoningEvent, SessionEvent, SessionFormat, SessionMetadata,
    ToolCallEvent, ToolResultEvent, UniversalSession,
};

pub struct DroidMaterialization {
    pub session_file: PathBuf,
    pub settings_file: Option<PathBuf>,
}

pub fn load(path: &Path) -> Result<UniversalSession> {
    let file = File::open(path)
        .with_context(|| format!("failed to open Droid session {}", path.display()))?;
    let reader = BufReader::new(file);

    let mut session = UniversalSession::new(Uuid::new_v4().to_string());
    session.metadata.source_format = Some(SessionFormat::Droid);

    for line in reader.lines() {
        let line = line.with_context(|| format!("failed to read {}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }

        let value: Value = serde_json::from_str(&line)
            .with_context(|| format!("invalid JSONL in {}", path.display()))?;
        let timestamp = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_datetime);
        update_time_bounds(&mut session.metadata, timestamp);

        match value.get("type").and_then(Value::as_str) {
            Some("session_start") => import_session_start(&mut session.metadata, &value),
            Some("message") => import_message(&mut session.events, &value),
            _ => {}
        }
    }

    import_settings(&mut session.metadata, path);

    if session.metadata.title.is_none() {
        session.metadata.title = derive_title(&session);
    }

    Ok(session)
}

fn import_session_start(metadata: &mut SessionMetadata, value: &Value) {
    if let Some(id) = value.get("id").and_then(Value::as_str) {
        metadata.session_id = id.to_string();
        metadata.original_session_id = Some(id.to_string());
        metadata.source_format = Some(SessionFormat::Droid);
    }
    if let Some(title) = value
        .get("sessionTitle")
        .or_else(|| value.get("title"))
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
    {
        metadata.title = Some(title.to_string());
    }
    if let Some(cwd) = value.get("cwd").and_then(Value::as_str) {
        metadata.cwd = Some(PathBuf::from(cwd));
    }
    if let Some(version) = value.get("version") {
        metadata
            .extra
            .insert("droid_session_version".to_string(), version.clone());
    }
    if let Some(owner) = value.get("owner") {
        metadata
            .extra
            .insert("droid_owner".to_string(), owner.clone());
    }
    if let Some(host_id) = value.get("hostId") {
        metadata
            .extra
            .insert("droid_host_id".to_string(), host_id.clone());
    }
}

fn import_settings(metadata: &mut SessionMetadata, session_path: &Path) {
    let settings_path = session_path.with_extension("settings.json");
    let Ok(text) = fs::read_to_string(settings_path) else {
        return;
    };
    let Ok(settings) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    metadata
        .extra
        .insert("droid_settings".to_string(), settings.clone());
    if let Some(model) = settings.get("model").and_then(Value::as_str) {
        metadata.model = Some(model.to_string());
    }
}

fn import_message(events: &mut Vec<SessionEvent>, value: &Value) {
    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_datetime);
    let id = value.get("id").and_then(Value::as_str).map(str::to_string);
    let parent_id = value
        .get("parentId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let Some(message) = value.get("message") else {
        return;
    };
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("user")
        .to_string();
    let content = message.get("content").cloned().unwrap_or(Value::Null);

    match content {
        Value::String(text) => {
            push_message(
                events,
                id,
                parent_id,
                role,
                timestamp,
                vec![ContentBlock::text("text", text)],
            );
        }
        Value::Array(blocks) => {
            let mut message_blocks = Vec::new();
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("thinking") => {
                        flush_message_blocks(
                            events,
                            &mut message_blocks,
                            id.clone(),
                            parent_id.clone(),
                            role.clone(),
                            timestamp,
                        );
                        if let Some(text) = block.get("thinking").and_then(Value::as_str) {
                            events.push(SessionEvent::Reasoning(ReasoningEvent {
                                id: id.clone(),
                                parent_id: parent_id.clone(),
                                timestamp,
                                summary: vec![text.to_string()],
                                metadata: BTreeMap::new(),
                            }));
                        }
                    }
                    Some("tool_use") => {
                        flush_message_blocks(
                            events,
                            &mut message_blocks,
                            id.clone(),
                            parent_id.clone(),
                            role.clone(),
                            timestamp,
                        );
                        let call_id = block
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        if call_id.is_empty() {
                            continue;
                        }
                        events.push(SessionEvent::ToolCall(ToolCallEvent {
                            id: id.clone(),
                            parent_id: parent_id.clone(),
                            call_id,
                            name: block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("tool")
                                .to_string(),
                            timestamp,
                            arguments: block.get("input").cloned().unwrap_or(Value::Null),
                            metadata: BTreeMap::new(),
                        }));
                    }
                    Some("tool_result") => {
                        flush_message_blocks(
                            events,
                            &mut message_blocks,
                            id.clone(),
                            parent_id.clone(),
                            role.clone(),
                            timestamp,
                        );
                        let call_id = block
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        if call_id.is_empty() {
                            continue;
                        }
                        events.push(SessionEvent::ToolResult(ToolResultEvent {
                            id: id.clone(),
                            parent_id: parent_id.clone(),
                            call_id,
                            timestamp,
                            output: block.get("content").cloned().unwrap_or(Value::Null),
                            is_error: block
                                .get("is_error")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            metadata: BTreeMap::new(),
                        }));
                    }
                    _ => message_blocks.push(normalize_block(&block)),
                }
            }
            flush_message_blocks(events, &mut message_blocks, id, parent_id, role, timestamp);
        }
        _ => {}
    }
}

fn push_message(
    events: &mut Vec<SessionEvent>,
    id: Option<String>,
    parent_id: Option<String>,
    role: String,
    timestamp: Option<DateTime<Utc>>,
    blocks: Vec<ContentBlock>,
) {
    if blocks.is_empty() {
        return;
    }
    events.push(SessionEvent::Message(MessageEvent {
        id,
        parent_id,
        role,
        timestamp,
        blocks,
        metadata: BTreeMap::new(),
    }));
}

fn flush_message_blocks(
    events: &mut Vec<SessionEvent>,
    blocks: &mut Vec<ContentBlock>,
    id: Option<String>,
    parent_id: Option<String>,
    role: String,
    timestamp: Option<DateTime<Utc>>,
) {
    if blocks.is_empty() {
        return;
    }
    push_message(
        events,
        id,
        parent_id,
        role,
        timestamp,
        std::mem::take(blocks),
    );
}

pub fn write(session: &UniversalSession, output: &Path) -> Result<PathBuf> {
    let materialization = plan_output(session, output);
    if let Some(parent) = materialization.session_file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let session_id = droid_session_id(&session.metadata.session_id);
    let cwd = session
        .metadata
        .cwd
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    let title = derive_title(session)
        .or_else(|| session.metadata.title.clone())
        .unwrap_or_else(|| "Imported session".to_string());

    let mut file = File::create(&materialization.session_file).with_context(|| {
        format!(
            "failed to create Droid session {}",
            materialization.session_file.display()
        )
    })?;

    write_json_line(
        &mut file,
        &json!({
            "type": "session_start",
            "id": session_id,
            "title": title,
            "sessionTitle": title,
            "owner": "transession",
            "version": 2,
            "cwd": cwd.display().to_string(),
        }),
    )?;

    let mut previous_id: Option<String> = None;
    for event in &session.events {
        let (timestamp, message) = match event {
            SessionEvent::Message(message) => {
                let role = if message.role == "assistant" {
                    "assistant"
                } else {
                    "user"
                };
                let content = encode_message_blocks(&message.blocks, &message.role);
                if content.is_null() {
                    continue;
                }
                (
                    message.timestamp,
                    json!({
                        "role": role,
                        "content": content,
                    }),
                )
            }
            SessionEvent::Reasoning(reasoning) => {
                let content = reasoning
                    .summary
                    .iter()
                    .map(|text| {
                        json!({
                            "type": "thinking",
                            "thinking": text,
                        })
                    })
                    .collect::<Vec<_>>();
                (
                    reasoning.timestamp,
                    json!({
                        "role": "assistant",
                        "content": content,
                    }),
                )
            }
            SessionEvent::ToolCall(call) => (
                call.timestamp,
                json!({
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": call.call_id,
                        "name": call.name,
                        "input": call.arguments,
                    }],
                }),
            ),
            SessionEvent::ToolResult(result) => (
                result.timestamp,
                json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": result.call_id,
                        "content": encode_tool_result_output(&result.output),
                        "is_error": result.is_error,
                    }],
                }),
            ),
        };

        let id = Uuid::new_v4().to_string();
        write_json_line(
            &mut file,
            &json!({
                "type": "message",
                "id": id,
                "parentId": previous_id,
                "timestamp": event_timestamp(timestamp),
                "message": message,
            }),
        )?;
        previous_id = Some(id);
    }

    write_json_line(
        &mut file,
        &json!({
            "type": "session_end",
            "id": Uuid::new_v4().to_string(),
            "timestamp": event_timestamp(session.metadata.updated_at),
        }),
    )?;

    if let Some(settings_file) = &materialization.settings_file {
        if let Some(parent) = settings_file.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let settings = session
            .metadata
            .extra
            .get("droid_settings")
            .cloned()
            .unwrap_or_else(|| {
                json!({
                    "model": session.metadata.model.clone().unwrap_or_else(|| "default".to_string()),
                })
            });
        let text = serde_json::to_string_pretty(&settings).context("failed to encode settings")?;
        fs::write(settings_file, text)
            .with_context(|| format!("failed to write {}", settings_file.display()))?;
    }

    Ok(materialization.session_file)
}

fn plan_output(session: &UniversalSession, output: &Path) -> DroidMaterialization {
    if output.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
        return DroidMaterialization {
            session_file: output.to_path_buf(),
            settings_file: None,
        };
    }

    let cwd = session
        .metadata
        .cwd
        .as_deref()
        .unwrap_or_else(|| Path::new("."));
    let slug = path_to_droid_slug(cwd);
    let session_id = droid_session_id(&session.metadata.session_id);
    let session_dir = output.join("sessions").join(slug);
    DroidMaterialization {
        session_file: session_dir.join(format!("{session_id}.jsonl")),
        settings_file: Some(session_dir.join(format!("{session_id}.settings.json"))),
    }
}

fn normalize_block(value: &Value) -> ContentBlock {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let text = value
        .get("text")
        .or_else(|| value.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string);
    ContentBlock {
        kind,
        text,
        data: Some(value.clone()),
    }
}

fn encode_message_blocks(blocks: &[ContentBlock], role: &str) -> Value {
    if blocks.is_empty() {
        return Value::Null;
    }

    let prefix = if role == "developer" {
        Some("[transession imported developer message]\n\n")
    } else {
        None
    };

    let encoded = blocks
        .iter()
        .map(|block| {
            let text = match (prefix, &block.text) {
                (Some(prefix), Some(text)) => format!("{prefix}{text}"),
                (_, Some(text)) => text.clone(),
                (_, None) => block
                    .data
                    .as_ref()
                    .map(json_to_string)
                    .unwrap_or_else(String::new),
            };
            json!({
                "type": "text",
                "text": text,
            })
        })
        .collect::<Vec<_>>();

    Value::Array(encoded)
}

fn encode_tool_result_output(output: &Value) -> Value {
    match output {
        Value::String(text) => Value::String(text.clone()),
        _ => Value::String(json_to_string(output)),
    }
}

fn parse_datetime(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

fn event_timestamp(timestamp: Option<DateTime<Utc>>) -> String {
    timestamp
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn write_json_line(file: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *file, value).context("failed to encode JSON line")?;
    writeln!(file).context("failed to write JSON line")
}

fn update_time_bounds(metadata: &mut SessionMetadata, timestamp: Option<DateTime<Utc>>) {
    if let Some(timestamp) = timestamp {
        metadata.created_at = Some(
            metadata
                .created_at
                .map_or(timestamp, |current| std::cmp::min(current, timestamp)),
        );
        metadata.updated_at = Some(
            metadata
                .updated_at
                .map_or(timestamp, |current| std::cmp::max(current, timestamp)),
        );
    }
}

fn derive_title(session: &UniversalSession) -> Option<String> {
    session.events.iter().find_map(|event| {
        if let SessionEvent::Message(message) = event {
            if message.role == "user" {
                let title = message
                    .blocks
                    .iter()
                    .filter_map(|block| block.text.as_deref())
                    .collect::<Vec<_>>()
                    .join(" ");
                let collapsed = collapse_whitespace(&title);
                if !collapsed.is_empty() {
                    return Some(collapsed.chars().take(80).collect());
                }
            }
        }
        None
    })
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn json_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn droid_session_id(candidate: &str) -> String {
    Uuid::parse_str(candidate)
        .map(|uuid| uuid.to_string())
        .unwrap_or_else(|_| Uuid::new_v4().to_string())
}

fn path_to_droid_slug(path: &Path) -> String {
    let rendered = path.to_string_lossy();
    let mut slug = String::with_capacity(rendered.len());
    for ch in rendered.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else {
            slug.push('-');
        }
    }
    if slug.starts_with('-') {
        slug
    } else {
        format!("-{slug}")
    }
}
