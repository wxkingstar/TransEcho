use prost::Message;
use tracing::debug;

use super::proto::ast::{TranslateRequest, TranslateResponse};
use super::proto::common::RequestMeta;
use super::proto::event::Type as EventType;
use super::proto::understanding::{Audio, User};

/// Configuration for a translation session
pub struct SessionConfig {
    pub app_key: String,
    pub access_key: String,
    pub resource_id: String,
    pub connection_id: String,
    pub session_id: String,
    pub mode: String,           // "s2t" or "s2s"
    pub source_language: String, // "en", "ja", "zh", etc.
    pub target_language: String,
    pub speaker_id: String,     // TTS voice, e.g. "zh_female_vv_uranus_bigtts"
}

/// Encode a StartSession request
pub fn encode_start_session(config: &SessionConfig) -> Vec<u8> {
    let req = TranslateRequest {
        request_meta: Some(RequestMeta {
            endpoint: config.resource_id.clone(),
            app_key: config.app_key.clone(),
            app_id: String::new(),
            resource_id: config.resource_id.clone(),
            connection_id: config.connection_id.clone(),
            session_id: config.session_id.clone(),
            sequence: 0,
        }),
        event: EventType::StartSession as i32,
        user: Some(User {
            uid: String::new(),
            did: String::new(),
            platform: std::env::consts::OS.to_string(),
            sdk_version: String::new(),
            app_version: String::new(),
        }),
        source_audio: Some(Audio {
            format: "wav".to_string(),
            codec: "raw".to_string(),
            rate: 16000,
            bits: 16,
            channel: 1,
            binary_data: vec![],
            // Other fields default
            data: String::new(),
            url: String::new(),
            url_type: String::new(),
            language: String::new(),
            tos_bucket: String::new(),
            tos_access_key: String::new(),
            audio_tos_object: String::new(),
            role_trn: String::new(),
        }),
        target_audio: Some(Audio {
            format: if config.mode == "s2s" {
                "pcm".to_string()
            } else {
                String::new()
            },
            rate: if config.mode == "s2s" { 24000 } else { 0 },
            bits: if config.mode == "s2s" { 32 } else { 0 },
            channel: if config.mode == "s2s" { 1 } else { 0 },
            // Other fields default
            codec: String::new(),
            binary_data: vec![],
            data: String::new(),
            url: String::new(),
            url_type: String::new(),
            language: String::new(),
            tos_bucket: String::new(),
            tos_access_key: String::new(),
            audio_tos_object: String::new(),
            role_trn: String::new(),
        }),
        request: Some(super::proto::ast::ReqParams {
            mode: config.mode.clone(),
            source_language: config.source_language.clone(),
            target_language: config.target_language.clone(),
            speaker_id: config.speaker_id.clone(),
            corpus: None,
        }),
        denoise: Some(false),
    };

    req.encode_to_vec()
}

/// Encode an audio frame as a TaskRequest
pub fn encode_audio_frame(
    session_id: &str,
    connection_id: &str,
    sequence: i32,
    pcm_data: &[i16],
) -> Vec<u8> {
    // Convert i16 PCM to raw bytes (little-endian)
    let mut bytes = Vec::with_capacity(pcm_data.len() * 2);
    for &sample in pcm_data {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }

    let req = TranslateRequest {
        request_meta: Some(RequestMeta {
            endpoint: String::new(),
            app_key: String::new(),
            app_id: String::new(),
            resource_id: String::new(),
            connection_id: connection_id.to_string(),
            session_id: session_id.to_string(),
            sequence,
        }),
        event: EventType::TaskRequest as i32,
        user: None,
        source_audio: Some(Audio {
            binary_data: bytes,
            // All other fields default/empty
            data: String::new(),
            url: String::new(),
            url_type: String::new(),
            format: String::new(),
            codec: String::new(),
            language: String::new(),
            rate: 0,
            bits: 0,
            channel: 0,
            tos_bucket: String::new(),
            tos_access_key: String::new(),
            audio_tos_object: String::new(),
            role_trn: String::new(),
        }),
        target_audio: None,
        request: None,
        denoise: None,
    };

    req.encode_to_vec()
}

/// Encode a FinishSession request
pub fn encode_finish_session(session_id: &str, connection_id: &str, sequence: i32) -> Vec<u8> {
    let req = TranslateRequest {
        request_meta: Some(RequestMeta {
            endpoint: String::new(),
            app_key: String::new(),
            app_id: String::new(),
            resource_id: String::new(),
            connection_id: connection_id.to_string(),
            session_id: session_id.to_string(),
            sequence,
        }),
        event: EventType::FinishSession as i32,
        user: None,
        source_audio: None,
        target_audio: None,
        request: None,
        denoise: None,
    };

    req.encode_to_vec()
}

/// Decoded translation event from the server
#[derive(Debug, Clone)]
pub enum TranslationEvent {
    SessionStarted,
    SessionFinished,
    SessionFailed { message: String },
    SourceSubtitle { text: String, is_final: bool, start_time: i32, end_time: i32 },
    TranslationSubtitle { text: String, is_final: bool, start_time: i32, end_time: i32 },
    TtsAudio { data: Vec<u8> },
    TtsSentenceEnd,
    Usage { message: String },
    AudioMuted { duration_ms: i32 },
    Unknown { event: i32 },
}

/// Decode a server response
pub fn decode_response(data: &[u8]) -> Result<TranslationEvent, prost::DecodeError> {
    let resp = TranslateResponse::decode(data)?;

    let event = EventType::try_from(resp.event).unwrap_or(EventType::None);

    debug!("Received event: {:?}, text: {:?}", event, resp.text);

    // Check for errors in response meta
    if let Some(ref meta) = resp.response_meta {
        if meta.status_code != 0 && meta.status_code != 20000000 {
            return Ok(TranslationEvent::SessionFailed {
                message: format!(
                    "Status {}: {}",
                    meta.status_code, meta.message
                ),
            });
        }
    }

    let result = match event {
        EventType::SessionStarted => TranslationEvent::SessionStarted,
        EventType::SessionFinished => TranslationEvent::SessionFinished,
        EventType::SessionFailed => TranslationEvent::SessionFailed {
            message: resp
                .response_meta
                .map(|m| m.message)
                .unwrap_or_default(),
        },
        EventType::SourceSubtitleStart | EventType::SourceSubtitleResponse => {
            TranslationEvent::SourceSubtitle {
                text: resp.text,
                is_final: false,
                start_time: resp.start_time,
                end_time: resp.end_time,
            }
        }
        EventType::SourceSubtitleEnd => TranslationEvent::SourceSubtitle {
            text: resp.text,
            is_final: true,
            start_time: resp.start_time,
            end_time: resp.end_time,
        },
        EventType::TranslationSubtitleStart | EventType::TranslationSubtitleResponse => {
            TranslationEvent::TranslationSubtitle {
                text: resp.text,
                is_final: false,
                start_time: resp.start_time,
                end_time: resp.end_time,
            }
        }
        EventType::TranslationSubtitleEnd => TranslationEvent::TranslationSubtitle {
            text: resp.text,
            is_final: true,
            start_time: resp.start_time,
            end_time: resp.end_time,
        },
        EventType::TtsResponse => TranslationEvent::TtsAudio { data: resp.data },
        EventType::TtsSentenceEnd => TranslationEvent::TtsSentenceEnd,
        EventType::UsageResponse => TranslationEvent::Usage {
            message: resp
                .response_meta
                .map(|m| m.message)
                .unwrap_or_default(),
        },
        EventType::AudioMuted => TranslationEvent::AudioMuted {
            duration_ms: resp.muted_duration_ms,
        },
        _ => TranslationEvent::Unknown {
            event: resp.event,
        },
    };

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::proto::common::ResponseMeta;

    #[test]
    fn test_encode_start_session() {
        let config = SessionConfig {
            app_key: "test-key".to_string(),
            access_key: "test-access".to_string(),
            resource_id: "volc.service_type.10053".to_string(),
            connection_id: "conn-1".to_string(),
            session_id: "sess-1".to_string(),
            mode: "s2t".to_string(),
            source_language: "en".to_string(),
            target_language: "zh".to_string(),
            speaker_id: String::new(),
        };
        let data = encode_start_session(&config);
        assert!(!data.is_empty());

        // Verify it can be decoded
        let req = TranslateRequest::decode(data.as_slice()).unwrap();
        assert_eq!(req.event, EventType::StartSession as i32);
        assert_eq!(req.request.unwrap().mode, "s2t");
        // Verify audio format
        let audio = req.source_audio.unwrap();
        assert_eq!(audio.format, "wav");
        assert_eq!(audio.rate, 16000);
    }

    #[test]
    fn test_encode_audio_frame() {
        // 80ms of 16kHz audio = 1280 samples
        let samples: Vec<i16> = vec![0; 1280];
        let data = encode_audio_frame("sess-1", "conn-1", 1, &samples);
        assert!(!data.is_empty());

        let req = TranslateRequest::decode(data.as_slice()).unwrap();
        assert_eq!(req.event, EventType::TaskRequest as i32);
        let audio = req.source_audio.unwrap();
        assert_eq!(audio.binary_data.len(), 2560); // 1280 * 2 bytes
    }

    #[test]
    fn test_encode_audio_frame_alignment() {
        let samples: Vec<i16> = (0..1280).map(|i| (i % 256) as i16).collect();
        let data = encode_audio_frame("s", "c", 0, &samples);
        let req = TranslateRequest::decode(data.as_slice()).unwrap();
        let audio_bytes = req.source_audio.unwrap().binary_data;
        assert_eq!(audio_bytes.len(), 2560);

        let first = i16::from_le_bytes([audio_bytes[0], audio_bytes[1]]);
        assert_eq!(first, 0);
        let second = i16::from_le_bytes([audio_bytes[2], audio_bytes[3]]);
        assert_eq!(second, 1);
    }

    #[test]
    fn test_decode_translation_response() {
        let resp = TranslateResponse {
            response_meta: Some(ResponseMeta {
                session_id: "s".to_string(),
                sequence: 1,
                status_code: 20000000,
                message: String::new(),
                billing: None,
            }),
            event: EventType::TranslationSubtitleResponse as i32,
            data: vec![],
            text: "你好世界".to_string(),
            start_time: 0,
            end_time: 1000,
            spk_chg: false,
            muted_duration_ms: 0,
        };
        let encoded = resp.encode_to_vec();
        let event = decode_response(&encoded).unwrap();
        match event {
            TranslationEvent::TranslationSubtitle { text, is_final, .. } => {
                assert_eq!(text, "你好世界");
                assert!(!is_final);
            }
            _ => panic!("Expected TranslationSubtitle"),
        }
    }

    #[test]
    fn test_decode_error_response() {
        let resp = TranslateResponse {
            response_meta: Some(ResponseMeta {
                session_id: "s".to_string(),
                sequence: 0,
                status_code: 45000001,
                message: "Invalid request".to_string(),
                billing: None,
            }),
            event: EventType::SessionFailed as i32,
            data: vec![],
            text: String::new(),
            start_time: 0,
            end_time: 0,
            spk_chg: false,
            muted_duration_ms: 0,
        };
        let encoded = resp.encode_to_vec();
        let event = decode_response(&encoded).unwrap();
        match event {
            TranslationEvent::SessionFailed { message } => {
                assert!(message.contains("45000001"));
            }
            _ => panic!("Expected SessionFailed"),
        }
    }

    #[test]
    fn test_decode_empty_response() {
        let resp = TranslateResponse {
            response_meta: None,
            event: EventType::None as i32,
            data: vec![],
            text: String::new(),
            start_time: 0,
            end_time: 0,
            spk_chg: false,
            muted_duration_ms: 0,
        };
        let encoded = resp.encode_to_vec();
        let event = decode_response(&encoded).unwrap();
        assert!(matches!(event, TranslationEvent::Unknown { .. }));
    }
}
