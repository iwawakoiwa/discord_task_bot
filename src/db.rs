use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};

use crate::models::{PendingReminder, Priority, Task, TaskStatus};

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    pub fn new(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn init(&self) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tasks (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id          TEXT NOT NULL,
                guild_id         TEXT NOT NULL,
                title            TEXT NOT NULL,
                description      TEXT,
                status           TEXT NOT NULL DEFAULT 'Pending',
                priority         TEXT NOT NULL DEFAULT 'Medium',
                due_date         TEXT,
                created_at       TEXT NOT NULL,
                channel_id       TEXT,
                remind_before    INTEGER,
                reminded         INTEGER NOT NULL DEFAULT 0,
                discord_event_id TEXT
            );
            CREATE TABLE IF NOT EXISTS reminders (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id       INTEGER NOT NULL,
                remind_before INTEGER NOT NULL,
                reminded      INTEGER NOT NULL DEFAULT 0,
                UNIQUE(task_id, remind_before)
            );",
        )?;

        // 旧カラム追加（既存DBへの互換マイグレーション）
        let _ = conn.execute("ALTER TABLE tasks ADD COLUMN channel_id TEXT", []);
        let _ = conn.execute("ALTER TABLE tasks ADD COLUMN remind_before INTEGER", []);
        let _ = conn.execute(
            "ALTER TABLE tasks ADD COLUMN reminded INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute("ALTER TABLE tasks ADD COLUMN discord_event_id TEXT", []);

        // 旧 remind_before データを reminders テーブルへ移行
        conn.execute_batch(
            "INSERT OR IGNORE INTO reminders (task_id, remind_before, reminded)
             SELECT id, remind_before, reminded
             FROM tasks
             WHERE remind_before IS NOT NULL;",
        )?;

        Ok(())
    }

    async fn call<F, R>(&self, f: F) -> Result<R, crate::Error>
    where
        F: FnOnce(&Connection) -> Result<R, crate::Error> + Send + 'static,
        R: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap();
            f(&guard)
        })
        .await
        .map_err(|e| -> crate::Error { Box::new(e) })?
    }

    pub async fn add_task(
        &self,
        user_id: String,
        guild_id: String,
        channel_id: String,
        title: String,
        description: Option<String>,
        priority: Priority,
        due_date: Option<String>,
        reminders: Vec<i64>,
    ) -> Result<i64, crate::Error> {
        let created_at = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
        self.call(move |conn| {
            conn.execute(
                "INSERT INTO tasks
                    (user_id, guild_id, channel_id, title, description, status,
                     priority, due_date, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'Pending', ?6, ?7, ?8)",
                params![
                    user_id, guild_id, channel_id, title, description,
                    priority.as_str(), due_date, created_at
                ],
            )
            .map_err(|e| -> crate::Error { Box::new(e) })?;

            let task_id = conn.last_insert_rowid();

            for secs in &reminders {
                conn.execute(
                    "INSERT OR IGNORE INTO reminders (task_id, remind_before, reminded)
                     VALUES (?1, ?2, 0)",
                    params![task_id, secs],
                )
                .map_err(|e| -> crate::Error { Box::new(e) })?;
            }

            Ok(task_id)
        })
        .await
    }

    pub async fn list_tasks(
        &self,
        guild_id: String,
        status_filter: Option<TaskStatus>,
    ) -> Result<Vec<Task>, crate::Error> {
        self.call(move |conn| {
            let base = "SELECT t.id, t.user_id, t.guild_id, t.title, t.description,
                                t.status, t.priority, t.due_date, t.created_at, t.channel_id,
                                GROUP_CONCAT(r.remind_before || ':' || r.reminded) AS reminders_data,
                                t.discord_event_id
                         FROM tasks t
                         LEFT JOIN reminders r ON r.task_id = t.id";
            let order = "GROUP BY t.id
                         ORDER BY CASE t.priority
                             WHEN 'High' THEN 1 WHEN 'Medium' THEN 2 ELSE 3 END,
                             t.created_at DESC";

            let tasks = if let Some(status) = status_filter {
                let sql = format!("{base} WHERE t.guild_id = ?1 AND t.status = ?2 {order}");
                let mut stmt = conn.prepare(&sql).map_err(|e| -> crate::Error { Box::new(e) })?;
                stmt.query_map(params![guild_id, status.as_str()], row_to_task)
                    .map_err(|e| -> crate::Error { Box::new(e) })?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| -> crate::Error { Box::new(e) })?
            } else {
                let sql = format!("{base} WHERE t.guild_id = ?1 {order}");
                let mut stmt = conn.prepare(&sql).map_err(|e| -> crate::Error { Box::new(e) })?;
                stmt.query_map(params![guild_id], row_to_task)
                    .map_err(|e| -> crate::Error { Box::new(e) })?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| -> crate::Error { Box::new(e) })?
            };
            Ok(tasks)
        })
        .await
    }

    pub async fn get_task(
        &self,
        id: i64,
        guild_id: String,
    ) -> Result<Option<Task>, crate::Error> {
        self.call(move |conn| {
            let sql = "SELECT t.id, t.user_id, t.guild_id, t.title, t.description,
                              t.status, t.priority, t.due_date, t.created_at, t.channel_id,
                              GROUP_CONCAT(r.remind_before || ':' || r.reminded) AS reminders_data,
                              t.discord_event_id
                       FROM tasks t
                       LEFT JOIN reminders r ON r.task_id = t.id
                       WHERE t.id = ?1 AND t.guild_id = ?2
                       GROUP BY t.id";
            let mut stmt = conn.prepare(sql).map_err(|e| -> crate::Error { Box::new(e) })?;
            let mut iter = stmt
                .query_map(params![id, guild_id], row_to_task)
                .map_err(|e| -> crate::Error { Box::new(e) })?;
            match iter.next() {
                Some(r) => Ok(Some(r.map_err(|e| -> crate::Error { Box::new(e) })?)),
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn update_task_status(
        &self,
        id: i64,
        guild_id: String,
        status: TaskStatus,
    ) -> Result<usize, crate::Error> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE tasks SET status = ?1 WHERE id = ?2 AND guild_id = ?3",
                params![status.as_str(), id, guild_id],
            )
            .map_err(|e| -> crate::Error { Box::new(e) })
        })
        .await
    }

    pub async fn delete_task(
        &self,
        id: i64,
        guild_id: String,
    ) -> Result<usize, crate::Error> {
        self.call(move |conn| {
            conn.execute("DELETE FROM reminders WHERE task_id = ?1", params![id])
                .map_err(|e| -> crate::Error { Box::new(e) })?;
            conn.execute(
                "DELETE FROM tasks WHERE id = ?1 AND guild_id = ?2",
                params![id, guild_id],
            )
            .map_err(|e| -> crate::Error { Box::new(e) })
        })
        .await
    }

    pub async fn edit_task(
        &self,
        id: i64,
        guild_id: String,
        title: Option<String>,
        description: Option<String>,
        priority: Option<Priority>,
        due_date: Option<String>,
        // None = リマインダーを変更しない, Some(vec) = 置き換え（空 vec = 全削除）
        reminders: Option<Vec<i64>>,
    ) -> Result<usize, crate::Error> {
        self.call(move |conn| {
            let result = conn.query_row(
                "SELECT title, description, priority, due_date
                 FROM tasks WHERE id = ?1 AND guild_id = ?2",
                params![id, guild_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            );

            let (cur_title, cur_desc, cur_priority, cur_due) = match result {
                Ok(v) => v,
                Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(0),
                Err(e) => return Err(Box::new(e) as crate::Error),
            };

            let due_changed = due_date.as_ref().map(|d| d != cur_due.as_deref().unwrap_or("")) == Some(true)
                || (due_date.is_some() && cur_due.is_none());

            let new_title = title.unwrap_or(cur_title);
            let new_desc = description.or(cur_desc);
            let new_priority = priority.map(|p| p.as_str().to_string()).unwrap_or(cur_priority);
            let new_due = due_date.or(cur_due);

            conn.execute(
                "UPDATE tasks SET title = ?1, description = ?2, priority = ?3, due_date = ?4
                 WHERE id = ?5 AND guild_id = ?6",
                params![new_title, new_desc, new_priority, new_due, id, guild_id],
            )
            .map_err(|e| -> crate::Error { Box::new(e) })?;

            if let Some(new_reminders) = reminders {
                // リマインダーを全置き換え
                conn.execute("DELETE FROM reminders WHERE task_id = ?1", params![id])
                    .map_err(|e| -> crate::Error { Box::new(e) })?;
                for secs in &new_reminders {
                    conn.execute(
                        "INSERT OR IGNORE INTO reminders (task_id, remind_before, reminded)
                         VALUES (?1, ?2, 0)",
                        params![id, secs],
                    )
                    .map_err(|e| -> crate::Error { Box::new(e) })?;
                }
            } else if due_changed {
                // 期限が変わったので全リマインダーをリセット
                conn.execute(
                    "UPDATE reminders SET reminded = 0 WHERE task_id = ?1",
                    params![id],
                )
                .map_err(|e| -> crate::Error { Box::new(e) })?;
            }

            Ok(1)
        })
        .await
    }

    pub async fn get_pending_reminders(&self) -> Result<Vec<PendingReminder>, crate::Error> {
        self.call(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT r.id, r.task_id, t.user_id, t.title, t.channel_id,
                            t.due_date, r.remind_before
                     FROM reminders r
                     JOIN tasks t ON t.id = r.task_id
                     WHERE r.reminded = 0
                       AND t.status != 'Done'
                       AND t.due_date IS NOT NULL
                       AND t.channel_id IS NOT NULL",
                )
                .map_err(|e| -> crate::Error { Box::new(e) })?;

            stmt.query_map([], |row| {
                Ok(PendingReminder {
                    reminder_id: row.get(0)?,
                    task_id: row.get(1)?,
                    user_id: row.get(2)?,
                    title: row.get(3)?,
                    channel_id: row.get(4)?,
                    due_date: row.get(5)?,
                    remind_before: row.get(6)?,
                })
            })
            .map_err(|e| -> crate::Error { Box::new(e) })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| -> crate::Error { Box::new(e) })
        })
        .await
    }

    pub async fn mark_reminder_sent(&self, reminder_id: i64) -> Result<(), crate::Error> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE reminders SET reminded = 1 WHERE id = ?1",
                params![reminder_id],
            )
            .map_err(|e| -> crate::Error { Box::new(e) })?;
            Ok(())
        })
        .await
    }

    pub async fn set_event_id(&self, task_id: i64, event_id: String) -> Result<(), crate::Error> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE tasks SET discord_event_id = ?1 WHERE id = ?2",
                params![event_id, task_id],
            )
            .map_err(|e| -> crate::Error { Box::new(e) })?;
            Ok(())
        })
        .await
    }
}

fn parse_reminders(s: Option<String>) -> Vec<(i64, bool)> {
    let s = match s {
        Some(s) if !s.is_empty() => s,
        _ => return vec![],
    };
    let mut entries: Vec<(i64, bool)> = s
        .split(',')
        .filter_map(|part| {
            let mut it = part.splitn(2, ':');
            let secs = it.next()?.parse::<i64>().ok()?;
            let reminded = it.next()?.parse::<i64>().ok()? != 0;
            Some((secs, reminded))
        })
        .collect();
    entries.sort_by_key(|(s, _)| *s);
    entries
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        user_id: row.get(1)?,
        guild_id: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        status: TaskStatus::from_str(&row.get::<_, String>(5)?),
        priority: Priority::from_str(&row.get::<_, String>(6)?),
        due_date: row.get(7)?,
        created_at: row.get(8)?,
        channel_id: row.get(9)?,
        reminders: parse_reminders(row.get(10)?),
        discord_event_id: row.get(11)?,
    })
}
