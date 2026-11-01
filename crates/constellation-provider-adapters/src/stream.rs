//! Bounded, incremental text-only stream decoding. This parser never dispatches
//! tools or exposes a response as complete before the protocol's terminal event.

use constellation_core::ProviderKind;
use serde_json::Value;

use super::{AdapterError, MAX_RESPONSE_BYTES, MAX_TEXT_BYTES};

/// A decoder can receive arbitrary HTTP chunk boundaries, including boundaries
/// in the middle of a UTF-8 code point. It only publishes complete text deltas.
pub struct StreamDecoder {
    kind: ProviderKind,
    line: Vec<u8>,
    event_name: String,
    event_data: String,
    text: String,
    bytes_received: usize,
    terminal: bool,
    stopped: bool,
    /// Once any chunk fails validation the stream is permanently invalid.
    /// A caller cannot discard the error and later commit earlier text.
    failure: Option<AdapterError>,
    blocks: std::collections::HashMap<u64, bool>,
}

impl StreamDecoder {
    pub fn new(kind: ProviderKind) -> Result<Self, AdapterError> {
        if kind == ProviderKind::GoogleGemini {
            return Err(AdapterError::UnsupportedContent);
        }
        Ok(Self {
            kind,
            line: Vec::new(),
            event_name: String::new(),
            event_data: String::new(),
            text: String::new(),
            bytes_received: 0,
            terminal: false,
            stopped: false,
            failure: None,
            blocks: std::collections::HashMap::new(),
        })
    }

    /// Returns safe text deltas accepted by the parser. Call `finish` before
    /// persisting or treating the response as successful: truncation, tool use,
    /// refusals and server errors can appear after an earlier text delta.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, AdapterError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let result = self.push_chunk(chunk);
        if let Err(error) = &result {
            self.failure = Some(*error);
        }
        result
    }

    fn push_chunk(&mut self, chunk: &[u8]) -> Result<Vec<String>, AdapterError> {
        self.bytes_received = self
            .bytes_received
            .checked_add(chunk.len())
            .filter(|size| *size <= MAX_RESPONSE_BYTES)
            .ok_or(AdapterError::TooLarge)?;
        if self.terminal && !chunk.is_empty() {
            return Err(AdapterError::InvalidResponse);
        }
        let mut output = Vec::new();
        for &byte in chunk {
            // The final blank line that closes [DONE]/message_stop is itself
            // valid, but no further byte belongs to this response, even when
            // appended to the same HTTP chunk.
            if self.terminal {
                return Err(AdapterError::InvalidResponse);
            }
            if byte == b'\n' {
                let mut line = std::mem::take(&mut self.line);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                let line = std::str::from_utf8(&line).map_err(|_| AdapterError::InvalidResponse)?;
                if self.terminal && !line.is_empty() {
                    return Err(AdapterError::InvalidResponse);
                }
                let part = if self.kind == ProviderKind::OllamaNative {
                    if line.trim().is_empty() {
                        None
                    } else {
                        self.decode_ollama(line)?
                    }
                } else {
                    self.decode_sse_line(line)?
                };
                if let Some(part) = part {
                    self.append(part, &mut output)?;
                }
            } else {
                self.line.push(byte);
                if self.line.len() > MAX_TEXT_BYTES {
                    return Err(AdapterError::TooLarge);
                }
            }
        }
        Ok(output)
    }

    pub fn finish(self) -> Result<String, AdapterError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        if !self.line.is_empty()
            || !self.event_data.is_empty()
            || !self.event_name.is_empty()
            || !self.terminal
            || self.text.trim().is_empty()
            || (self.kind == ProviderKind::AnthropicMessages && !self.stopped)
        {
            return Err(AdapterError::Incomplete);
        }
        Ok(self.text)
    }

    fn append(&mut self, part: String, output: &mut Vec<String>) -> Result<(), AdapterError> {
        if self.text.len().saturating_add(part.len()) > MAX_TEXT_BYTES {
            return Err(AdapterError::TooLarge);
        }
        if !part.is_empty() {
            self.text.push_str(&part);
            output.push(part);
        }
        Ok(())
    }

    fn decode_ollama(&mut self, line: &str) -> Result<Option<String>, AdapterError> {
        let data: Value = serde_json::from_str(line).map_err(|_| AdapterError::InvalidResponse)?;
        if data.get("error").is_some() {
            return Err(AdapterError::InvalidResponse);
        }
        if let Some(message) = data.get("message") {
            if message.get("role").and_then(Value::as_str) != Some("assistant") {
                return Err(AdapterError::UnsupportedContent);
            }
        } else if data.get("done") != Some(&Value::Bool(true)) {
            return Err(AdapterError::InvalidResponse);
        }
        if data.pointer("/message/tool_calls").is_some() {
            return Err(AdapterError::UnsupportedContent);
        }
        let part = data
            .pointer("/message/content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if data.get("done").and_then(Value::as_bool) == Some(true) {
            if data
                .get("done_reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| reason != "stop")
            {
                return Err(AdapterError::Incomplete);
            }
            self.terminal = true;
        } else if data.get("done") != Some(&Value::Bool(false)) {
            return Err(AdapterError::InvalidResponse);
        }
        Ok(Some(part))
    }

    fn decode_sse_line(&mut self, line: &str) -> Result<Option<String>, AdapterError> {
        if line.is_empty() {
            if self.event_data.is_empty() {
                self.event_name.clear();
                return Ok(None);
            }
            let data = std::mem::take(&mut self.event_data);
            let event = std::mem::take(&mut self.event_name);
            if data == "[DONE]" {
                if !matches!(
                    self.kind,
                    ProviderKind::OpenaiChat | ProviderKind::CustomCompatible
                ) || !self.stopped
                {
                    return Err(AdapterError::Incomplete);
                }
                self.terminal = true;
                return Ok(None);
            }
            let value: Value =
                serde_json::from_str(&data).map_err(|_| AdapterError::InvalidResponse)?;
            return match self.kind {
                ProviderKind::OpenaiChat | ProviderKind::CustomCompatible => {
                    self.decode_chat(&value)
                }
                ProviderKind::OpenaiResponses => self.decode_responses(&event, &value),
                ProviderKind::AnthropicMessages => self.decode_anthropic(&event, &value),
                _ => Err(AdapterError::UnsupportedContent),
            };
        }
        if line.starts_with(':') {
            return Ok(None);
        }
        if let Some(rest) = line.strip_prefix("event:") {
            if !self.event_name.is_empty() {
                return Err(AdapterError::InvalidResponse);
            }
            self.event_name = rest.trim().to_owned();
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !self.event_data.is_empty() {
                self.event_data.push('\n');
            }
            self.event_data
                .push_str(rest.strip_prefix(' ').unwrap_or(rest));
        }
        if self.event_data.len() > MAX_TEXT_BYTES {
            return Err(AdapterError::TooLarge);
        }
        Ok(None)
    }

    fn decode_chat(&mut self, v: &Value) -> Result<Option<String>, AdapterError> {
        if v.get("error").is_some() {
            return Err(AdapterError::InvalidResponse);
        }
        let choices = v
            .get("choices")
            .and_then(Value::as_array)
            .ok_or(AdapterError::InvalidResponse)?;
        if choices.is_empty() {
            // Optional terminal usage chunk contains no choices.
            return Ok(None);
        }
        if self.stopped {
            // A provider cannot append more content or another finish marker
            // after declaring the single assistant choice complete.
            return Err(AdapterError::InvalidResponse);
        }
        if choices.len() != 1 || choices[0].get("index").and_then(Value::as_u64) != Some(0) {
            return Err(AdapterError::UnsupportedContent);
        }
        let choice = &choices[0];
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            if reason != "stop" {
                return Err(AdapterError::Incomplete);
            }
            self.stopped = true;
        }
        let delta = choice.get("delta").ok_or(AdapterError::InvalidResponse)?;
        if delta.get("tool_calls").is_some()
            || delta.get("function_call").is_some()
            || delta.get("refusal").and_then(Value::as_str).is_some()
            || delta.get("audio").is_some()
        {
            return Err(AdapterError::UnsupportedContent);
        }
        if let Some(role) = delta.get("role").and_then(Value::as_str)
            && role != "assistant"
        {
            return Err(AdapterError::UnsupportedContent);
        }
        let text = delta.get("content").and_then(Value::as_str).unwrap_or("");
        Ok(Some(text.to_owned()))
    }

    fn decode_responses(&mut self, event: &str, v: &Value) -> Result<Option<String>, AdapterError> {
        if v.get("type").and_then(Value::as_str) != Some(event) {
            return Err(AdapterError::InvalidResponse);
        }
        match event {
            "response.output_text.delta" => Ok(Some(
                v.get("delta")
                    .and_then(Value::as_str)
                    .ok_or(AdapterError::InvalidResponse)?
                    .to_owned(),
            )),
            "response.completed" => {
                if v.pointer("/response/status").and_then(Value::as_str) != Some("completed") {
                    return Err(AdapterError::Incomplete);
                }
                if let Some(items) = v.pointer("/response/output").and_then(Value::as_array) {
                    let mut final_text = String::new();
                    for item in items {
                        match item.get("type").and_then(Value::as_str) {
                            Some("reasoning") => (),
                            Some("message") => {
                                if item.get("role").and_then(Value::as_str) != Some("assistant")
                                    || item.get("status").and_then(Value::as_str)
                                        != Some("completed")
                                {
                                    return Err(AdapterError::UnsupportedContent);
                                }
                                let blocks = item
                                    .get("content")
                                    .and_then(Value::as_array)
                                    .ok_or(AdapterError::InvalidResponse)?;
                                for block in blocks {
                                    if block.get("type").and_then(Value::as_str)
                                        != Some("output_text")
                                    {
                                        return Err(AdapterError::UnsupportedContent);
                                    }
                                    let text = block
                                        .get("text")
                                        .and_then(Value::as_str)
                                        .ok_or(AdapterError::InvalidResponse)?;
                                    if final_text.len().saturating_add(text.len()) > MAX_TEXT_BYTES
                                    {
                                        return Err(AdapterError::TooLarge);
                                    }
                                    final_text.push_str(text);
                                }
                            }
                            _ => return Err(AdapterError::UnsupportedContent),
                        }
                    }
                    if final_text != self.text {
                        return Err(AdapterError::InvalidResponse);
                    }
                }
                self.terminal = true;
                Ok(None)
            }
            "error" | "response.failed" => Err(AdapterError::InvalidResponse),
            "response.incomplete" => Err(AdapterError::Incomplete),
            "response.output_item.added" | "response.output_item.done" => {
                match v.pointer("/item/type").and_then(Value::as_str) {
                    Some("message" | "reasoning") => Ok(None),
                    _ => Err(AdapterError::UnsupportedContent),
                }
            }
            "response.content_part.added" | "response.content_part.done" => {
                if v.pointer("/part/type").and_then(Value::as_str) != Some("output_text") {
                    return Err(AdapterError::UnsupportedContent);
                }
                Ok(None)
            }
            "response.created" | "response.in_progress" | "response.output_text.done" => Ok(None),
            _ => Err(AdapterError::UnsupportedContent),
        }
    }

    fn decode_anthropic(&mut self, event: &str, v: &Value) -> Result<Option<String>, AdapterError> {
        if v.get("type").and_then(Value::as_str) != Some(event) {
            return Err(AdapterError::InvalidResponse);
        }
        if self.stopped && !matches!(event, "message_stop" | "ping") {
            return Err(AdapterError::InvalidResponse);
        }
        match event {
            "message_start" | "ping" => Ok(None),
            "content_block_start" => {
                let index = v
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or(AdapterError::InvalidResponse)?;
                let kind = v.pointer("/content_block/type").and_then(Value::as_str);
                match kind {
                    Some("text") => {
                        if self.blocks.insert(index, true).is_some() {
                            return Err(AdapterError::InvalidResponse);
                        }
                        // A completed text block can begin with nonempty text;
                        // accepting it here prevents silent loss of that text.
                        Ok(Some(
                            v.pointer("/content_block/text")
                                .and_then(Value::as_str)
                                .ok_or(AdapterError::InvalidResponse)?
                                .to_owned(),
                        ))
                    }
                    Some("thinking") => {
                        if self.blocks.insert(index, false).is_some() {
                            return Err(AdapterError::InvalidResponse);
                        }
                        Ok(None)
                    }
                    _ => Err(AdapterError::UnsupportedContent),
                }
            }
            "content_block_delta" => {
                let index = v
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or(AdapterError::InvalidResponse)?;
                let text_block = *self
                    .blocks
                    .get(&index)
                    .ok_or(AdapterError::InvalidResponse)?;
                let delta_type = v.pointer("/delta/type").and_then(Value::as_str);
                match (text_block, delta_type) {
                    (true, Some("text_delta")) => Ok(Some(
                        v.pointer("/delta/text")
                            .and_then(Value::as_str)
                            .ok_or(AdapterError::InvalidResponse)?
                            .to_owned(),
                    )),
                    (false, Some("thinking_delta" | "signature_delta")) => Ok(None),
                    _ => Err(AdapterError::UnsupportedContent),
                }
            }
            "content_block_stop" => {
                let index = v
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or(AdapterError::InvalidResponse)?;
                if self.blocks.remove(&index).is_none() {
                    return Err(AdapterError::InvalidResponse);
                }
                Ok(None)
            }
            "message_delta" => {
                let reason = v.pointer("/delta/stop_reason").and_then(Value::as_str);
                match reason {
                    Some("end_turn" | "stop_sequence") => self.stopped = true,
                    None => (),
                    _ => return Err(AdapterError::Incomplete),
                }
                Ok(None)
            }
            "message_stop" if self.stopped && self.blocks.is_empty() => {
                self.terminal = true;
                Ok(None)
            }
            "error" => Err(AdapterError::InvalidResponse),
            _ => Err(AdapterError::InvalidResponse),
        }
    }
}
