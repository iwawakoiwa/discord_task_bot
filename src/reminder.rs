use std::sync::Arc;

use chrono::Local;
use poise::serenity_prelude::{ChannelId, Http};

use crate::{db::Database, Error};

pub fn format_duration(secs: i64) -> String {
    if secs >= 7 * 24 * 3600 {
        format!("{}週間前", secs / (7 * 24 * 3600))
    } else if secs >= 24 * 3600 {
        format!("{}日前", secs / (24 * 3600))
    } else if secs >= 3600 {
        format!("{}時間前", secs / 3600)
    } else {
        format!("{}分前", secs / 60)
    }
}

fn parse_due_date(s: &str) -> Option<chrono::NaiveDateTime> {
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
        return Some(dt);
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(dt);
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return d.and_hms_opt(0, 0, 0);
    }
    None
}

pub async fn check_reminders(db: &Database, http: &Arc<Http>) -> Result<(), Error> {
    let pending = db.get_pending_reminders().await?;
    if pending.is_empty() {
        return Ok(());
    }

    let now = Local::now().naive_local();

    for reminder in pending {
        let due = match parse_due_date(&reminder.due_date) {
            Some(d) => d,
            None => {
                eprintln!(
                    "タスク #{} の期限形式が不正: {}",
                    reminder.task_id, reminder.due_date
                );
                continue;
            }
        };

        let notify_at = due - chrono::Duration::seconds(reminder.remind_before);
        if now < notify_at {
            continue;
        }

        let channel_id = match reminder.channel_id.parse::<u64>() {
            Ok(id) if id > 0 => ChannelId::new(id),
            _ => continue,
        };

        let time_label = format_duration(reminder.remind_before);
        let content = format!(
            "⏰ **タスクリマインダー**\n\
             <@{}> タスク「**{}**」の期限**{}**です！\n\
             📅 期限: `{}`",
            reminder.user_id, reminder.title, time_label, reminder.due_date,
        );

        if let Err(e) = channel_id.say(http, &content).await {
            eprintln!(
                "リマインダー送信エラー (reminder #{}): {:?}",
                reminder.reminder_id, e
            );
        } else {
            db.mark_reminder_sent(reminder.reminder_id).await?;
        }
    }

    Ok(())
}
