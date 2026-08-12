use serde_json::Value;
use sha2::{Digest, Sha256};

use super::provider::{ChatMessage, ChatRequest, ToolDef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixFingerprint {
    pub hash: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestFingerprint {
    pub system: PrefixFingerprint,
    pub schema: PrefixFingerprint,
    pub messages: PrefixFingerprint,
    message_hashes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixClassification {
    FirstRequest,
    AppendOnly,
    System,
    Schema,
    MessagePrefix,
}

impl PrefixClassification {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstRequest => "first_request",
            Self::AppendOnly => "append_only",
            Self::System => "system",
            Self::Schema => "schema",
            Self::MessagePrefix => "message_prefix",
        }
    }
}

pub fn fingerprint_request(req: &ChatRequest) -> RequestFingerprint {
    let system = fingerprint_value(&Value::String(req.system.clone()));
    let schema = fingerprint_value(&Value::Array(
        req.tools.iter().map(tool_value).collect(),
    ));
    let message_values: Vec<Value> = req.messages.iter().map(message_value).collect();
    let messages = fingerprint_value(&Value::Array(message_values.clone()));
    let message_hashes = message_values
        .iter()
        .map(|message| fingerprint_value(message).hash)
        .collect();
    RequestFingerprint {
        system,
        schema,
        messages,
        message_hashes,
    }
}

pub fn classify(
    previous: Option<&RequestFingerprint>,
    current: &RequestFingerprint,
) -> PrefixClassification {
    let Some(previous) = previous else {
        return PrefixClassification::FirstRequest;
    };
    if previous.system.hash != current.system.hash {
        return PrefixClassification::System;
    }
    if previous.schema.hash != current.schema.hash {
        return PrefixClassification::Schema;
    }
    if current.message_hashes.len() >= previous.message_hashes.len()
        && previous
            .message_hashes
            .iter()
            .zip(&current.message_hashes)
            .all(|(before, after)| before == after)
    {
        return PrefixClassification::AppendOnly;
    }
    PrefixClassification::MessagePrefix
}

fn tool_value(tool: &ToolDef) -> Value {
    serde_json::json!({
        "name": tool.name,
        "description": tool.description,
        "parameters": tool.parameters,
    })
}

fn message_value(message: &ChatMessage) -> Value {
    serde_json::json!({
        "role": message.role,
        "content": message.content,
        "tool_calls": message.tool_calls,
        "tool_call_id": message.tool_call_id,
    })
}

fn fingerprint_value(value: &Value) -> PrefixFingerprint {
    let canonical = canonical_json(value);
    let mut hasher = Sha256::new();
    hasher.update(b"xconsole-prefix-v1\0");
    hasher.update(canonical.as_bytes());
    PrefixFingerprint {
        hash: hex::encode(hasher.finalize()),
        bytes: canonical.len(),
    }
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<_> = object.keys().collect();
            keys.sort();
            let parts = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap(),
                        canonical_json(&object[key])
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(canonical_json).collect::<Vec<_>>().join(",")
        ),
        _ => serde_json::to_string(value).unwrap_or_else(|_| "null".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ChatRequest {
        let mut req = ChatRequest::new("test-model");
        req.system = "private prompt content".into();
        req.messages = vec![ChatMessage::user("hello")];
        req.tools = vec![ToolDef {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({ "type": "object", "properties": {} }),
        }];
        req
    }

    #[test]
    fn object_key_order_does_not_change_schema_hash() {
        let mut first = request();
        first.tools[0].parameters = serde_json::json!({ "b": 2, "a": 1 });
        let mut second = request();
        second.tools[0].parameters = serde_json::json!({ "a": 1, "b": 2 });
        assert_eq!(
            fingerprint_request(&first).schema.hash,
            fingerprint_request(&second).schema.hash
        );
    }

    #[test]
    fn classification_distinguishes_system_schema_and_append_only_messages() {
        let first = request();
        let mut appended = first.clone();
        appended.messages.push(ChatMessage::assistant("answer"));
        let first_fp = fingerprint_request(&first);
        assert_eq!(classify(None, &first_fp), PrefixClassification::FirstRequest);
        assert_eq!(
            classify(Some(&first_fp), &fingerprint_request(&appended)),
            PrefixClassification::AppendOnly
        );

        let mut changed_system = first.clone();
        changed_system.system.push_str(" changed");
        assert_eq!(
            classify(Some(&first_fp), &fingerprint_request(&changed_system)),
            PrefixClassification::System
        );

        let mut changed_schema = first.clone();
        changed_schema.tools[0].description.push_str(" changed");
        assert_eq!(
            classify(Some(&first_fp), &fingerprint_request(&changed_schema)),
            PrefixClassification::Schema
        );

        let mut changed_message = first.clone();
        changed_message.messages[0] = ChatMessage::user("rewritten earlier message");
        assert_eq!(
            classify(Some(&first_fp), &fingerprint_request(&changed_message)),
            PrefixClassification::MessagePrefix
        );
    }

    #[test]
    fn fingerprint_does_not_contain_prompt_text() {
        let fingerprint = fingerprint_request(&request());
        assert!(!fingerprint.system.hash.contains("private"));
        assert!(!fingerprint.messages.hash.contains("hello"));
    }
}
