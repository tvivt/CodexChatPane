use super::{database, now, RateLimits};
use crate::usage::context;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::HashSet,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

const HISTORY_URL: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/history";
const MIN_REFRESH: i64 = 60_000;

// Never derive Debug or include credentials in arguments, files, or diagnostic errors.
struct Credentials {
    token: String,
    account: String,
    scope: String,
}
impl Credentials {
    fn load(home: &Path) -> Result<Self, String> {
        let auth: Value = serde_json::from_slice(
            &std::fs::read(home.join("auth.json")).map_err(|_| "重置卡历史登录资料不可用")?,
        )
        .map_err(|_| "重置卡历史登录资料不可用")?;
        let tokens = &auth["tokens"];
        let token = tokens["access_token"]
            .as_str()
            .ok_or("重置卡历史需要登录")?;
        let account = tokens["account_id"].as_str().ok_or("重置卡历史缺少账号")?;
        if token.is_empty()
            || account.is_empty()
            || [token, account]
                .iter()
                .any(|s| s.bytes().any(|b| b < 32 || matches!(b, b'"' | b'\\')))
        {
            return Err("重置卡历史登录资料无效".into());
        }
        let subject = [tokens["id_token"].as_str(), Some(token)]
            .into_iter()
            .flatten()
            .find_map(|jwt| {
                let payload = URL_SAFE_NO_PAD.decode(jwt.split('.').nth(1)?).ok()?;
                let value: Value = serde_json::from_slice(&payload).ok()?;
                value["sub"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            })
            .ok_or("重置卡历史缺少用户归属")?;
        if context::account(home)? != account {
            return Err("重置卡历史账号尚未确认".into());
        }
        Ok(Self {
            token: token.into(),
            account: account.into(),
            scope: format!("{}|user:{subject}", context::identity(home)?),
        })
    }
    fn current(&self, home: &Path) -> bool {
        Self::load(home)
            .is_ok_and(|current| current.scope == self.scope && current.token == self.token)
    }
}

#[derive(Deserialize)]
struct Page {
    events: Vec<ServerEvent>,
    next_cursor: Option<String>,
}
#[derive(Deserialize)]
struct ServerEvent {
    id: String,
    kind: String,
    occurred_at: String,
}

fn fetch(credentials: &Credentials, cursor: Option<&str>) -> Result<Page, String> {
    let url = if let Some(cursor) = cursor {
        let encoded: String = cursor
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        format!("{HISTORY_URL}?cursor={encoded}")
    } else {
        HISTORY_URL.into()
    };
    #[cfg(windows)]
    let executable = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .ok_or("Windows 目录不可用")?
        .join("System32/curl.exe");
    #[cfg(not(windows))]
    let executable = std::path::PathBuf::from("curl");
    let mut command = Command::new(executable);
    command
        .args([
            "--disable",
            "--config",
            "-",
            "--silent",
            "--fail",
            "--max-time",
            "8",
            "--connect-timeout",
            "4",
            "--max-filesize",
            "1048576",
            "--proto",
            "=https",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "重置卡历史 HTTP 读取器不可用")?;
    let config = format!("url = \"{url}\"\nheader = \"Authorization: Bearer {}\"\nheader = \"ChatGPT-Account-Id: {}\"\nheader = \"Accept: application/json\"\n", credentials.token, credentials.account);
    let write = child
        .stdin
        .take()
        .ok_or("重置卡历史读取器无输入")?
        .write_all(config.as_bytes());
    if write.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("重置卡历史请求未能发送".into());
    }
    let output = child.wait_with_output().map_err(|_| "重置卡历史请求失败")?;
    if !output.status.success() || output.stdout.len() > 1_048_576 {
        return Err(format!(
            "重置卡历史接口暂不可用 (reader_exit={:?})",
            output.status.code()
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "重置卡历史响应格式无效".into())
}

pub(super) fn revision(home: &Path) -> Result<Option<(String, i64)>, String> {
    let Ok(auth) = Credentials::load(home) else {
        return Ok(None);
    };
    let count = database(home)?
        .query_row(
            "SELECT COUNT(*) FROM quota_card_history WHERE scope=?1",
            [&auth.scope],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(Some((auth.scope, count)))
}

#[derive(Clone)]
pub(super) struct CardUse {
    pub id: String,
    pub account: String,
    pub at: i64,
}
pub(super) fn used(home: &Path) -> Result<Vec<CardUse>, String> {
    let Ok(auth) = Credentials::load(home) else {
        return Ok(vec![]);
    };
    let db = database(home)?;
    let mut statement = db.prepare("SELECT event_id,occurred_at FROM quota_card_history WHERE scope=?1 AND kind='used' ORDER BY occurred_at,event_id").map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([&auth.scope], |r| {
            Ok(CardUse {
                id: r.get(0)?,
                account: auth.account.clone(),
                at: r.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn changed(before: &RateLimits, after: &RateLimits) -> bool {
    if serde_json::to_value(&before.reset_credits).ok()
        != serde_json::to_value(&after.reset_credits).ok()
    {
        return true;
    }
    [&after.primary, &after.secondary]
        .into_iter()
        .flatten()
        .any(|new| {
            [&before.primary, &before.secondary]
                .into_iter()
                .flatten()
                .any(|old| {
                    old.window_minutes == new.window_minutes
                        && new.used_percent + 1.0 <= old.used_percent
                })
        })
}

pub(super) fn disappeared(
    before: &super::Sample,
    after: &super::Sample,
    confirmed: &super::Sample,
) -> Vec<String> {
    let account = &before.value.account_id;
    if account.is_none()
        || account != &after.value.account_id
        || account != &confirmed.value.account_id
        || after.at <= before.until
        || confirmed.at <= after.at
    {
        return vec![];
    }
    let complete = |value: &RateLimits| {
        let cards = value.reset_credits.as_ref()?;
        let details = cards.credits.as_ref()?;
        let ids: HashSet<_> = details.iter().map(|c| &c.id).collect();
        let available: Vec<_> = details
            .iter()
            .filter(|c| c.status == "available")
            .cloned()
            .collect();
        (ids.len() == details.len()
            && !ids.contains(&String::new())
            && available.len() as u64 == cards.available_count)
            .then_some(available)
    };
    let (Some(old), Some(new), Some(later)) = (
        complete(&before.value),
        complete(&after.value),
        complete(&confirmed.value),
    ) else {
        return vec![];
    };
    if old.len() <= new.len()
        || new.len() != later.len()
        || new
            .iter()
            .any(|c| !old.iter().any(|o| o.id == c.id) || !later.iter().any(|o| o.id == c.id))
    {
        return vec![];
    }
    let gone: Vec<_> = old
        .iter()
        .filter(|old| !new.iter().any(|new| new.id == old.id))
        .collect();
    if gone.len() != 1
        || gone.len() != old.len() - new.len()
        || gone.iter().any(|c| {
            !c.expires_at
                .is_some_and(|at| at.saturating_mul(1000) > confirmed.at)
        })
    {
        return vec![];
    }
    gone.into_iter().map(|c| c.id.clone()).collect()
}

struct Sync {
    checked: i64,
    completed: i64,
    retry_until: i64,
    pending: bool,
    cursor: Option<String>,
    limits: Option<String>,
}

pub fn refresh(home: &Path, limits: &RateLimits) -> Result<bool, String> {
    let scope = Credentials::load(home)?.scope;
    let changed = std::cell::Cell::new(false);
    context::share(format!("reset-card-history:{scope}"), || {
        refresh_with(home, limits, now(), fetch).map(|value| changed.set(value))
    })?;
    Ok(changed.get())
}

fn refresh_with(
    home: &Path,
    limits: &RateLimits,
    at: i64,
    mut read: impl FnMut(&Credentials, Option<&str>) -> Result<Page, String>,
) -> Result<bool, String> {
    let auth = Credentials::load(home)?;
    if limits.account_id.as_deref() != Some(&auth.account) {
        return Err("重置卡历史与额度账号不一致".into());
    }
    let db = database(home)?;
    let saved = db.query_row("SELECT checked_at,completed_at,retry_until,pending,cursor,last_limits FROM quota_card_sync WHERE scope=?1", [&auth.scope], |r| Ok(Sync { checked:r.get(0)?, completed:r.get(1)?, retry_until:r.get(2)?, pending:r.get(3)?, cursor:r.get(4)?, limits:r.get(5)? })).optional().map_err(|e|e.to_string())?;
    let trigger = saved
        .as_ref()
        .and_then(|s| s.limits.as_deref())
        .and_then(|s| serde_json::from_str::<RateLimits>(s).ok())
        .is_none_or(|old| changed(&old, limits));
    let retry_until =
        saved
            .as_ref()
            .map_or(0, |s| s.retry_until)
            .max(if trigger && saved.is_some() {
                at + 5 * MIN_REFRESH
            } else {
                0
            });
    let pending = trigger
        || saved
            .as_ref()
            .is_none_or(|s| s.pending || at - s.completed >= 86_400_000);
    db.execute("INSERT INTO quota_card_sync VALUES(?1,0,0,?2,?3,NULL,?4) ON CONFLICT(scope) DO UPDATE SET retry_until=excluded.retry_until,pending=excluded.pending,last_limits=excluded.last_limits", params![auth.scope,retry_until,pending,serde_json::to_string(limits).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    if !pending
        || saved
            .as_ref()
            .is_some_and(|s| at >= s.checked && at - s.checked < MIN_REFRESH)
    {
        return Ok(false);
    }
    db.execute(
        "UPDATE quota_card_sync SET checked_at=?2 WHERE scope=?1",
        params![auth.scope, at],
    )
    .map_err(|e| e.to_string())?;
    let mut cursor = saved.and_then(|s| s.cursor);
    let mut cursors = HashSet::new();
    let mut changed = false;
    // Resume large or interrupted histories on the next poll rather than holding a worker indefinitely.
    for _ in 0..5 {
        if !auth.current(home) {
            return Err("账号已切换，重置卡历史响应已弃置".into());
        }
        let mut page = read(&auth, cursor.as_deref())?;
        page.next_cursor = page.next_cursor.filter(|cursor| !cursor.is_empty());
        if !auth.current(home) {
            return Err("账号已切换，重置卡历史响应已弃置".into());
        }
        if let Some(next) = &page.next_cursor {
            if next.len() > 4096 || Some(next) == cursor.as_ref() || !cursors.insert(next.clone()) {
                return Err("重置卡历史分页游标无效".into());
            }
        }
        let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
        for event in page.events {
            if event.id.is_empty() || event.id.len() > 256 || event.kind.len() > 64 {
                return Err("重置卡历史事件无效".into());
            }
            let occurred = time::OffsetDateTime::parse(
                &event.occurred_at,
                &time::format_description::well_known::Rfc3339,
            )
            .map_err(|_| "重置卡历史事件时间无效")?
            .unix_timestamp_nanos()
                / 1_000_000;
            let occurred = i64::try_from(occurred).map_err(|_| "重置卡历史事件时间越界")?;
            let old: Option<(String,i64)> = tx.query_row("SELECT kind,occurred_at FROM quota_card_history WHERE scope=?1 AND event_id=?2",params![auth.scope,event.id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(|e|e.to_string())?;
            if old
                .as_ref()
                .is_some_and(|old| old != &(event.kind.clone(), occurred))
            {
                return Err("重置卡历史同一事件存在冲突，保留原记录".into());
            }
            changed |= tx
                .execute(
                    "INSERT OR IGNORE INTO quota_card_history VALUES(?1,?2,?3,?4)",
                    params![auth.scope, event.id, event.kind, occurred],
                )
                .map_err(|e| e.to_string())?
                > 0;
        }
        cursor = page.next_cursor;
        tx.execute("UPDATE quota_card_sync SET cursor=?2,completed_at=CASE WHEN ?2 IS NULL THEN ?3 ELSE completed_at END,pending=?4 WHERE scope=?1",params![auth.scope,cursor,at,cursor.is_some() || retry_until>at]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        if cursor.is_none() {
            break;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests;
