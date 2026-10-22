use rusqlite::Connection;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default)]
pub(super) struct Questions {
    // shortcut: a dismissal without a reply or later user message has no verified local event.
    pending: HashMap<(String, usize), i64>,
    answered: HashSet<(String, usize)>,
    cleared_at: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(super) fn reply(call: &str, index: usize) -> Value {
        json!([{"type":"text", "text":format!("<send_user_message_question_reply>\n{}\n</send_user_message_question_reply>",
            json!([{"questionItemId":json!(["request_user_input_async",call,index]).to_string(),"answer":"yes"}]))}])
    }

    #[test]
    fn matches_each_question_and_keeps_answers_when_merging_stale_snapshots() {
        let mut questions = Questions::default();
        questions.add("call-a", 2, Some(100));
        questions.add("call-b", 1, Some(100));
        let old = questions.clone();
        questions.user_message(&reply("call-a", 0), Some(200));
        questions.user_message(&json!([{"text":"<send_user_message_question_reply>bad json</send_user_message_question_reply>"}]), Some(200));
        assert_eq!(questions.pending.len(), 2);
        questions.user_message(&reply("call-a", 1), Some(200));
        assert!(questions.is_pending());
        questions.resolve_call("call-b");
        questions.merge(&old);
        assert!(!questions.is_pending());
        questions.add("call-a", 2, Some(100));
        assert!(
            !questions.is_pending(),
            "duplicate question events cannot undo answers"
        );
    }

    #[test]
    fn ordinary_user_message_clears_old_questions_in_history_and_cache() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE thread_items(thread_id TEXT,item_type TEXT,created_at_ms INTEGER,rollout_ordinal INTEGER,item_json TEXT);").unwrap();
        let question =
            json!({"type":"agentMessage","id":"old","questions":[{"title":"Please sign in"}]})
                .to_string();
        connection
            .execute(
                "INSERT INTO thread_items VALUES('chat','agentMessage',1000,1,?1)",
                [&question],
            )
            .unwrap();
        let old = Questions::read(&connection, "chat").unwrap();
        assert!(old.is_pending());
        let message = json!({"type":"userMessage","content":[{"type":"text","text":"先这样吧，明天我再试登录"}]}).to_string();
        connection
            .execute(
                "INSERT INTO thread_items VALUES('chat','userMessage',2000,2,?1)",
                [&message],
            )
            .unwrap();
        let mut current = Questions::read(&connection, "chat").unwrap();
        assert!(
            !current.is_pending(),
            "ordinary user input supersedes earlier waiting questions"
        );
        current.merge(&old);
        assert!(
            !current.is_pending(),
            "stale cached questions cannot reappear"
        );
    }
    #[test]
    fn clearing_boundary_rejects_unseen_stale_questions_but_keeps_new_ones() {
        let mut old = Questions::default();
        old.add("old", 1, Some(1000));
        let mut latest = Questions::default();
        latest.user_message(
            &json!([{"type":"text","text":"Continue with the next task"}]),
            Some(2000),
        );
        let mut reversed = old.clone();
        reversed.merge(&latest);
        latest.merge(&old);
        assert!(!latest.is_pending());
        assert!(
            !reversed.is_pending(),
            "merge order cannot undo ordinary input"
        );
        latest.add("delayed-old", 1, Some(1500));
        assert!(!latest.is_pending());
        latest.add("new", 2, Some(3000));
        latest.user_message(
            &json!([{"type":"text","text":"Delayed duplicate user event"}]),
            Some(2000),
        );
        assert_eq!(
            latest.pending.len(),
            2,
            "an older input cannot clear a newer question"
        );
        latest.user_message(&reply("new", 0), Some(4000));
        assert_eq!(
            latest.pending.len(),
            1,
            "structured replies still clear only their own question"
        );
        latest.user_message(&json!([]), Some(5000));
        assert!(
            latest.is_pending(),
            "empty placeholders are not ordinary user input"
        );
        latest.user_message(
            &json!([{"type":"image","url":"image-placeholder"}]),
            Some(5000),
        );
        assert!(!latest.is_pending());
    }
}

impl Questions {
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    pub fn add(&mut self, id: &str, count: usize, at: Option<i64>) {
        let at = at.unwrap_or(0);
        if self.cleared_at.is_some_and(|cleared| at <= cleared) {
            return;
        }
        for index in 0..count {
            let key = (id.to_string(), index);
            if !self.answered.contains(&key) {
                self.pending
                    .entry(key)
                    .and_modify(|previous| *previous = (*previous).max(at))
                    .or_insert(at);
            }
        }
    }

    pub fn user_message(&mut self, content: &Value, at: Option<i64>) {
        let Some(parts) = content.as_array() else {
            return;
        };
        let structured_reply = parts.iter().any(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| {
                    text.trim()
                        .starts_with("<send_user_message_question_reply>")
                })
        });
        if !structured_reply {
            if parts.iter().any(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.trim().is_empty())
                    || matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("image" | "localImage" | "input_image")
                    )
            }) {
                self.cleared_at = self.cleared_at.max(at);
                self.pending.retain(|key, created| {
                    let keep = self.cleared_at.is_some_and(|cleared| *created > cleared);
                    if !keep {
                        self.answered.insert(key.clone());
                    }
                    keep
                });
            }
            return;
        }
        for part in parts {
            let Some(text) = part.get("text").and_then(Value::as_str) else {
                continue;
            };
            let Some(body) = text
                .trim()
                .strip_prefix("<send_user_message_question_reply>")
                .and_then(|text| text.strip_suffix("</send_user_message_question_reply>"))
            else {
                continue;
            };
            let Ok(Value::Array(replies)) = serde_json::from_str::<Value>(body.trim()) else {
                continue;
            };
            for reply in replies {
                let Some(id) = reply.get("questionItemId").and_then(Value::as_str) else {
                    continue;
                };
                let Ok(Value::Array(key)) = serde_json::from_str::<Value>(id) else {
                    continue;
                };
                if key.len() != 3
                    || !matches!(
                        key[0].as_str(),
                        Some("request_user_input_async" | "request_user_input")
                    )
                {
                    continue;
                }
                if let (Some(call), Some(index)) = (key[1].as_str(), key[2].as_u64()) {
                    if let Ok(index) = usize::try_from(index) {
                        let key = (call.to_string(), index);
                        self.pending.remove(&key);
                        self.answered.insert(key);
                    }
                }
            }
        }
    }

    pub fn resolve_call(&mut self, call: &str) {
        let keys: Vec<_> = self
            .pending
            .keys()
            .filter(|(id, _)| id == call)
            .cloned()
            .collect();
        for key in keys {
            self.pending.remove(&key);
            self.answered.insert(key);
        }
    }

    pub fn merge(&mut self, other: &Self) {
        self.cleared_at = self.cleared_at.max(other.cleared_at);
        self.answered.extend(other.answered.iter().cloned());
        for (key, at) in &other.pending {
            self.pending
                .entry(key.clone())
                .and_modify(|previous| *previous = (*previous).max(*at))
                .or_insert(*at);
        }
        self.pending.retain(|key, at| {
            !self.answered.contains(key) && self.cleared_at.is_none_or(|cleared| *at > cleared)
        });
    }

    pub fn item(&mut self, item: &Value, at: Option<i64>) {
        match item.get("type").and_then(Value::as_str) {
            Some("agentMessage" | "AgentMessage") => {
                if let (Some(id), Some(questions)) = (
                    item.get("id").and_then(Value::as_str),
                    item.get("questions").and_then(Value::as_array),
                ) {
                    self.add(id, questions.len(), at);
                }
            }
            Some("userMessage" | "UserMessage") => self.user_message(&item["content"], at),
            _ => {}
        }
    }

    pub fn read(connection: &Connection, thread: &str) -> Result<Self, String> {
        let supported: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('thread_items') WHERE name='item_json')", [], |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        if !supported {
            return Ok(Self::default());
        }
        let mut statement = connection
            .prepare(
                "SELECT item_json, created_at_ms FROM thread_items WHERE thread_id=?1 AND \
             ((item_type='agentMessage' AND json_array_length(item_json, '$.questions')>0) OR \
              item_type='userMessage') \
             ORDER BY rollout_ordinal",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([thread], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
            })
            .map_err(|error| error.to_string())?;
        let mut questions = Self::default();
        for row in rows {
            let (json, at) = row.map_err(|error| error.to_string())?;
            let value: Value = serde_json::from_str(&json).map_err(|error| error.to_string())?;
            questions.item(&value, at);
        }
        Ok(questions)
    }
}
