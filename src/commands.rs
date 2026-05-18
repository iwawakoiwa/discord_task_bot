use poise::serenity_prelude::{
    Colour, CreateEmbed, CreateEmbedFooter, CreateScheduledEvent, ScheduledEventId,
    ScheduledEventType, Timestamp,
};

use crate::{
    models::{Priority, TaskStatus},
    reminder::format_duration,
    Context, Error,
};

// ────────────────────────────────────────────────────────────────────────────
// /now
// ────────────────────────────────────────────────────────────────────────────

/// 現在の日時を表示する
#[poise::command(slash_command)]
pub async fn now(ctx: Context<'_>) -> Result<(), Error> {
    let unix = chrono::Utc::now().timestamp();
    let msg = format!(
        "🕐 **現在時刻**\n\
         日時: <t:{unix}:F>\n\
         相対: <t:{unix}:R>",
    );
    ctx.say(msg).await?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// Choice parameter enums
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, poise::ChoiceParameter)]
pub enum PriorityChoice {
    #[name = "低 🟢"]
    Low,
    #[name = "中 🟡"]
    Medium,
    #[name = "高 🔴"]
    High,
}

impl From<PriorityChoice> for Priority {
    fn from(c: PriorityChoice) -> Self {
        match c {
            PriorityChoice::Low => Priority::Low,
            PriorityChoice::Medium => Priority::Medium,
            PriorityChoice::High => Priority::High,
        }
    }
}

#[derive(Debug, poise::ChoiceParameter)]
pub enum StatusFilterChoice {
    #[name = "すべて"]
    All,
    #[name = "待機中 ⏳"]
    Pending,
    #[name = "進行中 🔄"]
    InProgress,
    #[name = "完了 ✅"]
    Done,
}

#[derive(Debug, poise::ChoiceParameter)]
pub enum StatusUpdateChoice {
    #[name = "待機中 ⏳"]
    Pending,
    #[name = "進行中 🔄"]
    InProgress,
    #[name = "完了 ✅"]
    Done,
}

impl From<StatusUpdateChoice> for TaskStatus {
    fn from(c: StatusUpdateChoice) -> Self {
        match c {
            StatusUpdateChoice::Pending => TaskStatus::Pending,
            StatusUpdateChoice::InProgress => TaskStatus::InProgress,
            StatusUpdateChoice::Done => TaskStatus::Done,
        }
    }
}

#[derive(Debug, poise::ChoiceParameter)]
pub enum RemindChoice {
    #[name = "30分前"]
    ThirtyMin,
    #[name = "1時間前"]
    OneHour,
    #[name = "3時間前"]
    ThreeHours,
    #[name = "1日前"]
    OneDay,
    #[name = "3日前"]
    ThreeDays,
    #[name = "1週間前"]
    OneWeek,
}

impl RemindChoice {
    pub fn to_seconds(&self) -> i64 {
        match self {
            Self::ThirtyMin => 30 * 60,
            Self::OneHour => 3600,
            Self::ThreeHours => 3 * 3600,
            Self::OneDay => 24 * 3600,
            Self::ThreeDays => 3 * 24 * 3600,
            Self::OneWeek => 7 * 24 * 3600,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Helper
// ────────────────────────────────────────────────────────────────────────────

macro_rules! require_guild {
    ($ctx:expr) => {
        match $ctx.guild_id() {
            Some(id) => id.to_string(),
            None => {
                $ctx.send(
                    poise::CreateReply::default()
                        .ephemeral(true)
                        .content("このコマンドはサーバー内でのみ使用できます。"),
                )
                .await?;
                return Ok(());
            }
        }
    };
}

fn collect_reminders(
    r1: Option<RemindChoice>,
    r2: Option<RemindChoice>,
    r3: Option<RemindChoice>,
) -> Vec<i64> {
    let mut secs: Vec<i64> = [r1, r2, r3]
        .into_iter()
        .flatten()
        .map(|r| r.to_seconds())
        .collect();
    secs.sort();
    secs.dedup();
    secs
}

fn format_reminders(reminders: &[(i64, bool)]) -> String {
    if reminders.is_empty() {
        return "なし".to_string();
    }
    reminders
        .iter()
        .map(|(secs, reminded)| {
            if *reminded {
                format!("~~⏰ {}~~ ✔", format_duration(*secs))
            } else {
                format!("⏰ {}", format_duration(*secs))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn due_to_timestamp(due_date: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let naive = if let Ok(dt) =
        chrono::NaiveDateTime::parse_from_str(due_date, "%Y-%m-%d %H:%M")
    {
        dt
    } else if let Ok(dt) =
        chrono::NaiveDateTime::parse_from_str(due_date, "%Y-%m-%d %H:%M:%S")
    {
        dt
    } else if let Ok(d) = chrono::NaiveDate::parse_from_str(due_date, "%Y-%m-%d") {
        d.and_hms_opt(0, 0, 0)?
    } else {
        return None;
    };
    Some(chrono::DateTime::from_naive_utc_and_offset(naive, chrono::Utc))
}

// ────────────────────────────────────────────────────────────────────────────
// /help
// ────────────────────────────────────────────────────────────────────────────

/// コマンド一覧と使い方を表示する
#[poise::command(slash_command)]
pub async fn help(ctx: Context<'_>) -> Result<(), Error> {
    let embed = CreateEmbed::new()
        .title("📖 タスクボット コマンド一覧")
        .colour(Colour(0x5865F2))
        .field(
            "/task add",
            "タスクを追加する\n\
             `title` タイトル（必須）\n\
             `description` 説明\n\
             `priority` 優先度（低/中/高、デフォルト: 中）\n\
             `due_date` 期限（例: `2025-12-31 15:00`）\n\
             `remind1〜3` リマインダー（30分前〜1週間前）\n\
             `create_event` Discord イベントも作成する（true/false）",
            false,
        )
        .field(
            "/task list",
            "タスク一覧を表示する\n\
             `filter` ステータスで絞り込み（すべて/待機中/進行中/完了）",
            false,
        )
        .field(
            "/task view",
            "タスクの詳細を表示する\n\
             `id` タスクID（必須）",
            false,
        )
        .field(
            "/task status",
            "タスクのステータスを変更する\n\
             `id` タスクID（必須）\n\
             `new_status` 新しいステータス（待機中/進行中/完了）",
            false,
        )
        .field(
            "/task edit",
            "タスクを編集する\n\
             `id` タスクID（必須）\n\
             `title` / `description` / `priority` / `due_date` 各項目\n\
             `remind1〜3` を1つでも指定するとリマインダーが全置き換えされる",
            false,
        )
        .field(
            "/task delete",
            "タスクを削除する（紐づく Discord イベントも削除）\n\
             `id` タスクID（必須）",
            false,
        )
        .field(
            "/now",
            "現在の日時を表示する",
            false,
        )
        .field(
            "期限の書式",
            "`YYYY-MM-DD HH:MM` （例: `2025-12-31 09:00`）\n\
             時刻を省略すると `00:00` 扱い",
            false,
        );

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// Command group
// ────────────────────────────────────────────────────────────────────────────

/// タスク管理コマンド
#[poise::command(
    slash_command,
    subcommands("add", "list", "view", "status", "edit", "delete")
)]
pub async fn task(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// /task add
// ────────────────────────────────────────────────────────────────────────────

/// 新しいタスクを追加する
#[poise::command(slash_command)]
pub async fn add(
    ctx: Context<'_>,
    #[description = "タスクのタイトル"] title: String,
    #[description = "タスクの説明"] description: Option<String>,
    #[description = "優先度 (デフォルト: 中)"] priority: Option<PriorityChoice>,
    #[description = "期限 (例: 2024-12-31 15:00)"] due_date: Option<String>,
    #[description = "リマインダー1"] remind1: Option<RemindChoice>,
    #[description = "リマインダー2"] remind2: Option<RemindChoice>,
    #[description = "リマインダー3"] remind3: Option<RemindChoice>,
    #[description = "Discord スケジュールイベントも作成する (デフォルト: false)"]
    create_event: Option<bool>,
) -> Result<(), Error> {
    let guild_id = require_guild!(ctx);

    let reminders = collect_reminders(remind1, remind2, remind3);
    if !reminders.is_empty() && due_date.is_none() {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("リマインダーを設定するには期限 (due_date) も指定してください。"),
        )
        .await?;
        return Ok(());
    }

    if create_event == Some(true) && due_date.is_none() {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("イベントを作成するには期限 (due_date) も指定してください。"),
        )
        .await?;
        return Ok(());
    }

    let user_id = ctx.author().id.to_string();
    let channel_id = ctx.channel_id().to_string();
    let priority = priority.map(Priority::from).unwrap_or(Priority::Medium);

    let task_id = ctx
        .data()
        .db
        .add_task(
            user_id,
            guild_id.clone(),
            channel_id,
            title.clone(),
            description.clone(),
            priority.clone(),
            due_date.clone(),
            reminders.clone(),
        )
        .await?;

    // Discord スケジュールイベント作成
    let mut event_url: Option<String> = None;
    if create_event == Some(true) {
        if let Some(ref due) = due_date {
            if let Some(start_dt) = due_to_timestamp(due) {
                let end_dt = start_dt + chrono::Duration::hours(1);
                let ts = Timestamp::from_unix_timestamp(start_dt.timestamp()).unwrap();
                let ts_end = Timestamp::from_unix_timestamp(end_dt.timestamp()).unwrap();
                let guild_id_u64: u64 = guild_id.parse().unwrap_or(0);
                let serenity_guild = poise::serenity_prelude::GuildId::new(guild_id_u64);
                let event_builder = CreateScheduledEvent::new(
                    ScheduledEventType::External,
                    title.clone(),
                    ts,
                )
                .description(description.as_deref().unwrap_or(""))
                .location(title.clone())
                .end_time(ts_end);

                match serenity_guild
                    .create_scheduled_event(ctx.serenity_context(), event_builder)
                    .await
                {
                    Ok(event) => {
                        let eid = event.id.to_string();
                        event_url = Some(format!(
                            "https://discord.com/events/{}/{}",
                            guild_id, eid
                        ));
                        ctx.data().db.set_event_id(task_id, eid).await?;
                    }
                    Err(e) => {
                        eprintln!("イベント作成エラー: {:?}", e);
                    }
                }
            }
        }
    }

    let remind_str = if reminders.is_empty() {
        "なし".to_string()
    } else {
        reminders
            .iter()
            .map(|s| format!("⏰ {}", format_duration(*s)))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mut embed = CreateEmbed::new()
        .title("✅ タスクを追加しました")
        .colour(Colour(0x2ECC71))
        .field("ID", task_id.to_string(), true)
        .field("タイトル", &title, true)
        .field(
            "優先度",
            format!("{} {}", priority.emoji(), priority.display()),
            true,
        )
        .field("説明", description.as_deref().unwrap_or("なし"), false)
        .field("期限", due_date.as_deref().unwrap_or("未設定"), true)
        .field("リマインダー", remind_str, true);

    if let Some(ref url) = event_url {
        embed = embed.field("イベント", format!("[Discord イベント]({})", url), false);
    }

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// /task list
// ────────────────────────────────────────────────────────────────────────────

/// タスク一覧を表示する
#[poise::command(slash_command)]
pub async fn list(
    ctx: Context<'_>,
    #[description = "絞り込み (デフォルト: すべて)"] filter: Option<StatusFilterChoice>,
) -> Result<(), Error> {
    let guild_id = require_guild!(ctx);

    let status_filter = match filter {
        None | Some(StatusFilterChoice::All) => None,
        Some(StatusFilterChoice::Pending) => Some(TaskStatus::Pending),
        Some(StatusFilterChoice::InProgress) => Some(TaskStatus::InProgress),
        Some(StatusFilterChoice::Done) => Some(TaskStatus::Done),
    };

    let tasks = ctx
        .data()
        .db
        .list_tasks(guild_id, status_filter)
        .await?;

    if tasks.is_empty() {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("タスクはありません。`/task add` でタスクを追加してください。"),
        )
        .await?;
        return Ok(());
    }

    let mut embed = CreateEmbed::new()
        .title("📋 タスク一覧")
        .colour(Colour(0x3498DB));

    for task in &tasks {
        let has_pending_reminder = task.reminders.iter().any(|(_, reminded)| !reminded);
        let remind_icon = if has_pending_reminder { "⏰ " } else { "" };
        let reminded_all = !task.reminders.is_empty()
            && task.reminders.iter().all(|(_, reminded)| *reminded);

        let value = format!(
            "{} {} | <@{}> | 期限: {}{}",
            task.status.emoji(),
            task.priority.emoji(),
            task.user_id,
            task.due_date.as_deref().unwrap_or("未設定"),
            if reminded_all { " ✔" } else { "" },
        );
        embed = embed.field(
            format!("[{}] {}{}", task.id, remind_icon, task.title),
            value,
            false,
        );
    }

    embed = embed.footer(CreateEmbedFooter::new(format!("合計: {} 件", tasks.len())));

    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// /task view
// ────────────────────────────────────────────────────────────────────────────

/// タスクの詳細を表示する
#[poise::command(slash_command)]
pub async fn view(
    ctx: Context<'_>,
    #[description = "タスクID"] id: i64,
) -> Result<(), Error> {
    let guild_id = require_guild!(ctx);

    match ctx.data().db.get_task(id, guild_id.clone()).await? {
        Some(task) => {
            let color = match task.status {
                TaskStatus::Pending => Colour(0xF1C40F),
                TaskStatus::InProgress => Colour(0x3498DB),
                TaskStatus::Done => Colour(0x2ECC71),
            };

            let event_value = task.discord_event_id.as_ref().map(|eid| {
                format!(
                    "[Discord イベント](https://discord.com/events/{}/{})",
                    guild_id, eid
                )
            });

            let mut embed = CreateEmbed::new()
                .title(format!("📌 タスク #{}", task.id))
                .colour(color)
                .field("タイトル", &task.title, false)
                .field(
                    "ステータス",
                    format!("{} {}", task.status.emoji(), task.status.display()),
                    true,
                )
                .field(
                    "優先度",
                    format!("{} {}", task.priority.emoji(), task.priority.display()),
                    true,
                )
                .field("説明", task.description.as_deref().unwrap_or("なし"), false)
                .field("期限", task.due_date.as_deref().unwrap_or("未設定"), true)
                .field("作成者", format!("<@{}>", task.user_id), true)
                .field("リマインダー", format_reminders(&task.reminders), true)
                .field("作成日時", &task.created_at, true);

            if let Some(ev) = event_value {
                embed = embed.field("イベント", ev, false);
            }

            ctx.send(poise::CreateReply::default().embed(embed)).await?;
        }
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .ephemeral(true)
                    .content(format!("タスク #{} が見つかりません。", id)),
            )
            .await?;
        }
    }

    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// /task status
// ────────────────────────────────────────────────────────────────────────────

/// タスクのステータスを更新する
#[poise::command(slash_command)]
pub async fn status(
    ctx: Context<'_>,
    #[description = "タスクID"] id: i64,
    #[description = "新しいステータス"] new_status: StatusUpdateChoice,
) -> Result<(), Error> {
    let guild_id = require_guild!(ctx);
    let task_status = TaskStatus::from(new_status);
    let display = format!("{} {}", task_status.emoji(), task_status.display());

    let updated = ctx
        .data()
        .db
        .update_task_status(id, guild_id, task_status)
        .await?;

    if updated == 0 {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content(format!("タスク #{} が見つかりません。", id)),
        )
        .await?;
    } else {
        ctx.say(format!(
            "タスク #{} のステータスを {} に更新しました！",
            id, display
        ))
        .await?;
    }

    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// /task edit
// ────────────────────────────────────────────────────────────────────────────

/// タスクを編集する（remind を1つでも指定するとリマインダーが全置き換えされます）
#[poise::command(slash_command)]
pub async fn edit(
    ctx: Context<'_>,
    #[description = "タスクID"] id: i64,
    #[description = "新しいタイトル"] title: Option<String>,
    #[description = "新しい説明"] description: Option<String>,
    #[description = "新しい優先度"] priority: Option<PriorityChoice>,
    #[description = "新しい期限 (例: 2024-12-31 15:00)"] due_date: Option<String>,
    #[description = "リマインダー1 (指定するとリマインダーが全置き換え)"] remind1: Option<RemindChoice>,
    #[description = "リマインダー2"] remind2: Option<RemindChoice>,
    #[description = "リマインダー3"] remind3: Option<RemindChoice>,
) -> Result<(), Error> {
    let guild_id = require_guild!(ctx);

    let remind_changed = remind1.is_some() || remind2.is_some() || remind3.is_some();
    if title.is_none()
        && description.is_none()
        && priority.is_none()
        && due_date.is_none()
        && !remind_changed
    {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content("変更する項目を少なくとも1つ指定してください。"),
        )
        .await?;
        return Ok(());
    }

    let priority = priority.map(Priority::from);
    let reminders = if remind_changed {
        Some(collect_reminders(remind1, remind2, remind3))
    } else {
        None
    };

    let updated = ctx
        .data()
        .db
        .edit_task(id, guild_id, title, description, priority, due_date, reminders)
        .await?;

    if updated == 0 {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content(format!("タスク #{} が見つかりません。", id)),
        )
        .await?;
    } else {
        ctx.say(format!("✏️ タスク #{} を更新しました！", id)).await?;
    }

    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// /task delete
// ────────────────────────────────────────────────────────────────────────────

/// タスクを削除する
#[poise::command(slash_command)]
pub async fn delete(
    ctx: Context<'_>,
    #[description = "タスクID"] id: i64,
) -> Result<(), Error> {
    let guild_id = require_guild!(ctx);

    // イベント ID を先に取得しておく
    let event_id = ctx
        .data()
        .db
        .get_task(id, guild_id.clone())
        .await?
        .and_then(|t| t.discord_event_id);

    let deleted = ctx.data().db.delete_task(id, guild_id.clone()).await?;

    if deleted == 0 {
        ctx.send(
            poise::CreateReply::default()
                .ephemeral(true)
                .content(format!("タスク #{} が見つかりません。", id)),
        )
        .await?;
        return Ok(());
    }

    // Discord イベントも削除
    if let Some(eid) = event_id {
        if let Ok(eid_u64) = eid.parse::<u64>() {
            let guild_id_u64: u64 = guild_id.parse().unwrap_or(0);
            let serenity_guild = poise::serenity_prelude::GuildId::new(guild_id_u64);
            if let Err(e) = serenity_guild
                .delete_scheduled_event(ctx.serenity_context(), ScheduledEventId::new(eid_u64))
                .await
            {
                eprintln!("イベント削除エラー: {:?}", e);
            }
        }
    }

    ctx.say(format!("🗑️ タスク #{} を削除しました。", id)).await?;
    Ok(())
}
