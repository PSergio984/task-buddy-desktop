use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    pub aligned: bool,
    pub confidence: f32,
    pub source: String, // "groq_api" | "offline_heuristic" | "cache" | "empty_bypass"
}

#[derive(Clone)]
pub struct ContentMatcher {
    api_key: Arc<Mutex<Option<String>>>,
    cache: Arc<Mutex<HashMap<(String, String), MatchResult>>>,
    client: reqwest::Client,
}

impl Default for ContentMatcher {
    fn default() -> Self {
        Self {
            api_key: Arc::new(Mutex::new(None)),
            cache: Arc::new(Mutex::new(HashMap::new())),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }
}

impl ContentMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_api_key(&self, key: Option<String>) {
        let mut lock = self.api_key.lock().unwrap();
        *lock = key.filter(|k| !k.trim().is_empty());
        let mut cache = self.cache.lock().unwrap();
        cache.clear();
    }

    pub fn get_api_key(&self) -> Option<String> {
        self.api_key.lock().unwrap().clone()
    }

    /// Primary entry point: checks cache, then Groq API if key present, else offline heuristic
    pub async fn is_aligned(&self, task_title: &str, media_title: &str) -> MatchResult {
        let clean_task = task_title.trim();
        let clean_media = media_title.trim();

        if clean_task.is_empty() || clean_media.is_empty() {
            return MatchResult {
                aligned: true,
                confidence: 1.0,
                source: "empty_bypass".to_string(),
            };
        }

        let cache_key = (clean_task.to_lowercase(), clean_media.to_lowercase());

        // 1. Check in-memory cache
        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached) = cache.get(&cache_key) {
                return cached.clone();
            }
        }

        let key = self.get_api_key();

        // 2. Try Groq API if key is available
        let result = if let Some(api_key) = key {
            match self.evaluate_with_groq(&api_key, clean_task, clean_media).await {
                Ok(res) => res,
                Err(_) => self.evaluate_offline(clean_task, clean_media),
            }
        } else {
            self.evaluate_offline(clean_task, clean_media)
        };

        // Cache successful Groq API evaluations
        if result.source == "groq_api" {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(cache_key, result.clone());
        }

        result
    }

    /// Evaluates alignment for native OS monitor loop without delaying polling intervals.
    /// Checks cache first; if missing and Groq API is configured, kicks off background async
    /// cache population while immediately returning offline heuristic fallback.
    pub fn evaluate_monitor_alignment(&self, task_title: &str, media_title: &str) -> MatchResult {
        let clean_task = task_title.trim();
        let clean_media = media_title.trim();

        if clean_task.is_empty() || clean_media.is_empty() {
            return MatchResult {
                aligned: true,
                confidence: 1.0,
                source: "empty_bypass".to_string(),
            };
        }

        let cache_key = (clean_task.to_lowercase(), clean_media.to_lowercase());

        // 1. Check in-memory cache
        {
            let cache = self.cache.lock().unwrap();
            if let Some(cached) = cache.get(&cache_key) {
                return cached.clone();
            }
        }

        // 2. If Groq key is present, schedule background query to populate cache
        if self.get_api_key().is_some() {
            let matcher_clone = self.clone();
            let task_s = clean_task.to_string();
            let media_s = clean_media.to_string();
            tauri::async_runtime::spawn(async move {
                let _ = matcher_clone.is_aligned(&task_s, &media_s).await;
            });
        }

        // 3. Fallback to immediate offline heuristic
        self.evaluate_offline(clean_task, clean_media)
    }

    /// Calls Groq API with gpt-oss-20b or llama-3.1-8b-instant
    async fn evaluate_with_groq(
        &self,
        api_key: &str,
        task_title: &str,
        media_title: &str,
    ) -> Result<MatchResult, String> {
        let task_json = serde_json::to_string(task_title).unwrap_or_else(|_| format!("{:?}", task_title));
        let media_json = serde_json::to_string(media_title).unwrap_or_else(|_| format!("{:?}", media_title));
        let prompt = format!(
            "Compare the declared task title and the active video/tutorial title. Is the video content relevant or aligned with working on the task?\nTask: {}\nVideo: {}\nRespond ONLY in JSON format: {{\"aligned\": boolean, \"confidence\": float between 0.0 and 1.0}}",
            task_json, media_json
        );

        let body = serde_json::json!({
            "model": "openai/gpt-oss-20b",
            "messages": [
                {
                    "role": "system",
                    "content": "You are a focus assistant. Determine whether the video title is on-topic for the user's declared task. Output strict JSON."
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "response_format": { "type": "json_object" },
            "temperature": 0.1,
            "max_tokens": 60
        });

        let res = self
            .client
            .post("https://api.groq.com/openai/v1/chat/completions")
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !res.status().is_success() {
            return Err(format!("Groq API status: {}", res.status()));
        }

        #[derive(Deserialize)]
        struct ChatChoice {
            message: MessageContent,
        }
        #[derive(Deserialize)]
        struct MessageContent {
            content: String,
        }
        #[derive(Deserialize)]
        struct ChatResp {
            choices: Vec<ChatChoice>,
        }
        #[derive(Deserialize)]
        struct MatchPayload {
            aligned: bool,
            confidence: Option<f32>,
        }

        let chat_resp: ChatResp = res.json().await.map_err(|e| e.to_string())?;
        let first = chat_resp
            .choices
            .first()
            .ok_or_else(|| "No choices returned from Groq".to_string())?;

        let parsed: MatchPayload =
            serde_json::from_str(&first.message.content).map_err(|e| e.to_string())?;

        Ok(MatchResult {
            aligned: parsed.aligned,
            confidence: parsed.confidence.unwrap_or(0.9),
            source: "groq_api".to_string(),
        })
    }

    /// Fast offline Jaccard token overlap heuristic
    pub fn evaluate_offline(&self, task_title: &str, media_title: &str) -> MatchResult {
        let task_tokens = self.tokenize(task_title);
        let media_tokens = self.tokenize(media_title);

        if task_tokens.is_empty() || media_tokens.is_empty() {
            return MatchResult {
                aligned: false,
                confidence: 0.0,
                source: "offline_heuristic".to_string(),
            };
        }

        let intersection_count = task_tokens.intersection(&media_tokens).count();
        let union_count = task_tokens.union(&media_tokens).count();

        let jaccard = if union_count > 0 {
            intersection_count as f32 / union_count as f32
        } else {
            0.0
        };

        // If at least one meaningful keyword matches or overlap >= 0.20
        let aligned = intersection_count >= 1 || jaccard >= 0.20;

        MatchResult {
            aligned,
            confidence: jaccard,
            source: "offline_heuristic".to_string(),
        }
    }

    fn tokenize(&self, text: &str) -> HashSet<String> {
        let stop_words: HashSet<&'static str> = [
            "the", "a", "an", "and", "or", "in", "on", "at", "to", "for", "of", "with", "by",
            "how", "what", "tutorial", "video", "ep", "part",
        ]
        .into_iter()
        .collect();

        let short_tech_tokens: HashSet<&'static str> = [
            "c", "r", "go", "c++", "c#", "ui", "ai", "db", "os", "ip", "js", "ts", "py", "rs",
        ]
        .into_iter()
        .collect();

        text.split(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| {
                if s.is_empty() {
                    return false;
                }
                if short_tech_tokens.contains(s.as_str()) {
                    return true;
                }
                s.len() >= 3 && !stop_words.contains(s.as_str())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offline_matching_aligned() {
        let matcher = ContentMatcher::new();
        let res = matcher.evaluate_offline("Implement Rust Tauri backend", "Rust Tauri tutorial: Windows desktop development");
        assert!(res.aligned, "Expected 'Rust' and 'Tauri' to match");
        assert!(res.confidence > 0.0);
        assert_eq!(res.source, "offline_heuristic");
    }

    #[test]
    fn test_offline_matching_misaligned() {
        let matcher = ContentMatcher::new();
        let res = matcher.evaluate_offline("Refactor React components", "Top 10 funny cat videos 2026");
        assert!(!res.aligned, "Cat videos should not match React components");
        assert_eq!(res.confidence, 0.0);
    }

    #[test]
    fn test_short_tech_tokens() {
        let matcher = ContentMatcher::new();
        let res = matcher.evaluate_offline("Learn C++ templates", "Advanced C++ tutorial");
        assert!(res.aligned, "Expected 'c++' to match");
    }

    #[test]
    fn test_empty_titles_bypass() {
        let matcher = ContentMatcher::new();
        let res = tauri::async_runtime::block_on(matcher.is_aligned("", "Rust tutorial"));
        assert!(res.aligned);
        assert_eq!(res.source, "empty_bypass");
    }

    #[test]
    fn test_cache_hits() {
        let matcher = ContentMatcher::new();
        let key = (
            "write sqlite migrations".to_string(),
            "sqlite migrations in rust".to_string(),
        );
        matcher.cache.lock().unwrap().insert(
            key,
            MatchResult {
                aligned: true,
                confidence: 0.99,
                source: "groq_api".to_string(),
            },
        );

        let res = tauri::async_runtime::block_on(
            matcher.is_aligned("Write SQLite migrations", "SQLite migrations in Rust"),
        );
        assert!(res.aligned);
        assert_eq!(res.source, "groq_api");
        assert_eq!(res.confidence, 0.99);
        assert_eq!(matcher.cache.lock().unwrap().len(), 1);
    }
}
