use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionPhaseIn {
    pub kind: String, // "focus" | "break"
    pub minutes: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionStartPayload {
    pub mode: String, // "timer" | "stopwatch"
    pub phases: Vec<SessionPhaseIn>,
    pub task_id: Option<i64>,
    pub activity_label: Option<String>,
    pub difficulty: String,
    pub auto_start_breaks: bool,
    pub auto_start_focus: bool,
}

#[derive(Debug, Clone)]
pub struct BackendSyncClient {
    base_url: String,
    client: reqwest::Client,
}

impl Default for BackendSyncClient {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8000/api/v1".to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl BackendSyncClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn start_remote_session(
        &self,
        token: &str,
        minutes: u32,
        task_title: &str,
    ) -> Result<i64, String> {
        let payload = SessionStartPayload {
            mode: "timer".to_string(),
            phases: vec![SessionPhaseIn {
                kind: "focus".to_string(),
                minutes,
            }],
            task_id: None,
            activity_label: Some(task_title.to_string()),
            difficulty: "easy".to_string(),
            auto_start_breaks: false,
            auto_start_focus: false,
        };

        let res = self
            .client
            .post(format!("{}/game/sessions", self.base_url))
            .bearer_auth(token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(format!("Backend error {}: {}", status, text));
        }

        #[derive(Deserialize)]
        struct StartResp {
            session: SessionInfo,
        }
        #[derive(Deserialize)]
        struct SessionInfo {
            id: i64,
        }

        let body: StartResp = res
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        Ok(body.session.id)
    }

    pub async fn stop_remote_session(&self, token: &str, session_id: i64) -> Result<(), String> {
        let res = self
            .client
            .post(format!("{}/game/sessions/{}/stop", self.base_url, session_id))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(format!("Backend error {}: {}", status, text));
        }

        Ok(())
    }

    pub async fn record_diversion_penalty(
        &self,
        token: &str,
        session_id: Option<i64>,
        reason: &str,
        base_damage: f32,
    ) -> Result<DiversionPenaltyResult, String> {
        let payload = serde_json::json!({
            "session_id": session_id,
            "reason": reason,
            "base_damage": base_damage
        });

        let res = self
            .client
            .post(format!("{}/game/sessions/diversion", self.base_url))
            .bearer_auth(token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            return Err(format!("Backend error {}: {}", status, text));
        }

        let body: DiversionPenaltyResult = res
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        Ok(body)
    }

    pub async fn fetch_active_session_snapshot(
        &self,
        token: &str,
    ) -> Result<Option<RemoteSessionState>, String> {
        let res = self
            .client
            .get(format!("{}/game/sessions/stream?once=true", self.base_url))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
                return Err(format!("Backend auth expired or invalid: {}", status));
            }
            return Ok(None);
        }

        let text = res.text().await.unwrap_or_default();
        for line in text.lines() {
            if let Some(json_str) = line.strip_prefix("data:") {
                if let Ok(state) = serde_json::from_str::<RemoteSessionState>(json_str.trim()) {
                    return Ok(Some(state));
                }
            }
        }

        Ok(None)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiversionPenaltyResult {
    pub applied: bool,
    pub base_damage: f32,
    pub hp_lost: f32,
    pub damage_mitigated: f32,
    pub current_hp: f32,
    pub died: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSessionState {
    pub is_active: bool,
    pub session_id: Option<i64>,
    pub task_id: Option<i64>,
    pub activity_label: Option<String>,
    pub mode: Option<String>,
    pub status: String,
    pub current_phase: Option<RemotePhaseInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePhaseInfo {
    pub seq: i32,
    pub kind: String, // "focus" | "break"
    pub status: String,
    pub minutes: u32,
}

